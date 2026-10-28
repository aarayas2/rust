//! Reverse-engineered Win32 File I/O and Edit Control buffer synchronization.
//!
//! # Reverse Engineering Notes
//!
//! ## 1. `LoadFile` (`0x140010F48` - `0x140011867`) & `ReadFile` (`0x14000CA93`)
//! - **File Handle:** Opened at `0x14000DEAA` via `CreateFileW(path, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL)`.
//! - **BOM Inspection (`0x140011173` - `0x1400111D5`):**
//!   - `0xFEFF` (`FF FE` little-endian): UTF-16 LE (`encoding = 2`)
//!   - `0xFFFE` (`FE FF` little-endian): UTF-16 BE (`encoding = 3`)
//!   - `0xBBEF` + `0xBF` (`EF BB BF`): UTF-8 with BOM (`encoding = 4`, CodePage `0xFDE9` = 65001)
//!   - Default: UTF-8 without BOM (`encoding = 5`, CodePage `0xFDE9` = 65001)
//! - **Null Sanitization (`0x1400115AE` - `0x1400115C0`):**
//!   Replaces any interior `0x0000` wide character with `0x0020` (`' '`) so the Win32
//!   `Edit` control does not truncate the buffer at an embedded NUL.
//! - **`.LOG` Header Detection (`0x1400115C2` - `0x1400115E5`):**
//!   Checks if the first four wide characters are `'.' (0x2E)`, `'L' (0x4C)`, `'O' (0x4F)`, `'G' (0x47)`.
//!   If matched, sets `fLog = 1` to append the current time/date on load.
//! - **Reset Modify State (`0x14001161B` - `0x140011627`):**
//!   Sends `EM_SETMODIFY` (`0x00B9`) with `wParam = 0` to `g_hwndEdit`.
//!
//! ## 2. `SaveFile` (`0x140011AF8` - `0x140011F20`)
//! - **File Creation (`0x140011BE8`):**
//!   Calls `CreateFileW(path, GENERIC_READ | GENERIC_WRITE (0xC0000000), FILE_SHARE_READ, NULL, OPEN_ALWAYS (4), FILE_ATTRIBUTE_NORMAL (0x80), NULL)`.
//! - **Buffer Extraction (`0x140011C8A` - `0x140011CC6`):**
//!   Queries `WM_GETTEXTLENGTH` (`0x0E`), locks the Edit control buffer (`EM_GETHANDLE` `0xBD` + `LocalLock`),
//!   encodes according to the active `TextEncoding`, writes BOM + payload via `WriteFile`
//!   (`0x140011D1C`, `0x140011E16`, `0x140011E70`, `0x140011EC7`, `0x140011EEB`, `0x140010D1F`),
//!   and calls `SetEndOfFile` (`0x140011F0A`) to truncate any leftover trailing bytes.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetFileSizeEx, ReadFile, SetEndOfFile, WriteFile, FILE_ATTRIBUTE_NORMAL,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_ALWAYS, OPEN_EXISTING,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowTextLengthW, GetWindowTextW, SendMessageW, SetWindowTextW,
};

/// Edit control message to query or reset the modified (dirty) flag (`0x00B9` at `0x14001161B`).
pub const EM_GETMODIFY: u32 = 0x00B8;
pub const EM_SETMODIFY: u32 = 0x00B9;

/// Supported text encodings in Notepad (`0x140035620` / `0x140035624`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    /// UTF-8 without BOM (ID = 5, default in Windows 10/11 Notepad)
    Utf8,
    /// UTF-8 with BOM `EF BB BF` (ID = 4)
    Utf8Bom,
    /// UTF-16 Little Endian with BOM `FF FE` (ID = 2)
    Utf16Le,
    /// UTF-16 Big Endian with BOM `FE FF` (ID = 3)
    Utf16Be,
}

/// Result of loading a document from disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedDocument {
    pub text: String,
    pub encoding: TextEncoding,
    /// True if the document starts with `.LOG` (`0x1400115C2`).
    pub is_log_file: bool,
}

/// RAII wrapper around a Win32 `HANDLE` ensuring `CloseHandle` is always called.
struct OwnedFileHandle(HANDLE);

impl Drop for OwnedFileHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// Reads a file from disk using Win32 `CreateFileW` and `ReadFile`, performing
/// Notepad's BOM detection, null-byte sanitization, and `.LOG` prefix check.
pub fn load_file_win32(path: &Path) -> io::Result<LoadedDocument> {
    let wide_path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide_path.as_ptr()),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
        .map_err(|e| io::Error::from_raw_os_error(e.code().0))?
    };
    let file_guard = OwnedFileHandle(handle);

    let mut file_size: i64 = 0;
    unsafe {
        GetFileSizeEx(file_guard.0, &mut file_size)
            .map_err(|e| io::Error::from_raw_os_error(e.code().0))?;
    }

    // Notepad limits files to < 1 GiB (cmp r13d, 0x40000000 at 0x140011059)
    if file_size >= 0x4000_0000 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "File is too large for Notepad (> 1 GiB)",
        ));
    }

    let len = file_size as usize;
    let mut raw_bytes = vec![0u8; len];
    if len > 0 {
        let mut bytes_read: u32 = 0;
        unsafe {
            ReadFile(
                file_guard.0,
                Some(&mut raw_bytes),
                Some(&mut bytes_read),
                None,
            )
            .map_err(|e| io::Error::from_raw_os_error(e.code().0))?;
        }
        raw_bytes.truncate(bytes_read as usize);
    }

    Ok(decode_notepad_bytes(&raw_bytes))
}

/// Decodes raw file bytes following Notepad's BOM detection (`0x140011173`),
/// interior NUL replacement (`0x1400115AE`), and `.LOG` header detection (`0x1400115C2`).
pub fn decode_notepad_bytes(raw_bytes: &[u8]) -> LoadedDocument {
    let (mut text, encoding) = if raw_bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        let s = String::from_utf8_lossy(&raw_bytes[3..]).into_owned();
        (s, TextEncoding::Utf8Bom)
    } else if raw_bytes.starts_with(&[0xFF, 0xFE]) {
        let u16_words: Vec<u16> = raw_bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        (String::from_utf16_lossy(&u16_words), TextEncoding::Utf16Le)
    } else if raw_bytes.starts_with(&[0xFE, 0xFF]) {
        let u16_words: Vec<u16> = raw_bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        (String::from_utf16_lossy(&u16_words), TextEncoding::Utf16Be)
    } else {
        let s = String::from_utf8_lossy(raw_bytes).into_owned();
        (s, TextEncoding::Utf8)
    };

    // Sanitize embedded NUL characters to spaces (0x1400115AE: mov word ptr [rax], 0x20)
    if text.contains('\0') {
        text = text.replace('\0', " ");
    }

    // Normalize lone LF to CRLF for Win32 EDIT control compatibility
    let normalized = normalize_crlf(&text);

    // Check for ".LOG" prefix (0x1400115C2: '.', 'L', 'O', 'G')
    let is_log_file = normalized.starts_with(".LOG");

    LoadedDocument {
        text: normalized,
        encoding,
        is_log_file,
    }
}

/// Writes text to disk using Win32 `CreateFileW`, `WriteFile`, and `SetEndOfFile`,
/// matching `SaveFile` (`0x140011AF8`).
pub fn save_file_win32(path: &Path, text: &str, encoding: TextEncoding) -> io::Result<()> {
    let wide_path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // 0x140011BE8: CreateFileW with GENERIC_READ | GENERIC_WRITE (0xC0000000) and OPEN_ALWAYS (4)
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide_path.as_ptr()),
            (GENERIC_READ | GENERIC_WRITE).0,
            FILE_SHARE_READ,
            None,
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
        .map_err(|e| io::Error::from_raw_os_error(e.code().0))?
    };
    let file_guard = OwnedFileHandle(handle);

    let encoded_bytes = encode_notepad_bytes(text, encoding);

    let mut total_written = 0usize;
    while total_written < encoded_bytes.len() {
        let mut chunk_written: u32 = 0;
        let remaining = &encoded_bytes[total_written..];
        unsafe {
            WriteFile(
                file_guard.0,
                Some(remaining),
                Some(&mut chunk_written),
                None,
            )
            .map_err(|e| io::Error::from_raw_os_error(e.code().0))?;
        }
        if chunk_written == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "WriteFile wrote 0 bytes",
            ));
        }
        total_written += chunk_written as usize;
    }

    // 0x140011F0A: Truncate file at current end position via SetEndOfFile
    unsafe {
        SetEndOfFile(file_guard.0).map_err(|e| io::Error::from_raw_os_error(e.code().0))?;
    }

    Ok(())
}

/// Encodes text into raw bytes with appropriate BOM matching `0x140011CE1` - `0x140011EEB`.
pub fn encode_notepad_bytes(text: &str, encoding: TextEncoding) -> Vec<u8> {
    match encoding {
        TextEncoding::Utf8 => text.as_bytes().to_vec(),
        TextEncoding::Utf8Bom => {
            let mut out = Vec::with_capacity(3 + text.len());
            out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            out.extend_from_slice(text.as_bytes());
            out
        }
        TextEncoding::Utf16Le => {
            let mut out = Vec::with_capacity(2 + text.len() * 2);
            out.extend_from_slice(&[0xFF, 0xFE]);
            for w in text.encode_utf16() {
                out.extend_from_slice(&w.to_le_bytes());
            }
            out
        }
        TextEncoding::Utf16Be => {
            // Mirrors the byte-swap loop at 0x140011E35 - 0x140011E51
            let mut out = Vec::with_capacity(2 + text.len() * 2);
            out.extend_from_slice(&[0xFE, 0xFF]);
            for w in text.encode_utf16() {
                out.extend_from_slice(&w.to_be_bytes());
            }
            out
        }
    }
}

/// Reads the current text buffer from the child `"Edit"` control (`0x140011C8A`).
pub fn get_edit_text(hwnd_edit: HWND) -> String {
    if hwnd_edit.is_invalid() {
        return String::new();
    }
    unsafe {
        let len = GetWindowTextLengthW(hwnd_edit);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; (len as usize) + 1];
        let copied = GetWindowTextW(hwnd_edit, &mut buf);
        String::from_utf16_lossy(&buf[..copied as usize])
    }
}

/// Populates the child `"Edit"` control with text and clears the modified flag (`0x140011627`).
pub fn set_edit_text(hwnd_edit: HWND, text: &str) {
    if hwnd_edit.is_invalid() {
        return;
    }
    let normalized = normalize_crlf(text);
    let wide: Vec<u16> = normalized.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = SetWindowTextW(hwnd_edit, PCWSTR(wide.as_ptr()));
        // Clear dirty state: SendMessageW(hwnd_edit, EM_SETMODIFY, 0, 0)
        SendMessageW(hwnd_edit, EM_SETMODIFY, Some(WPARAM(0)), Some(LPARAM(0)));
    }
}

/// Queries whether the child `"Edit"` control has unsaved modifications (`EM_GETMODIFY`).
pub fn is_edit_modified(hwnd_edit: HWND) -> bool {
    if hwnd_edit.is_invalid() {
        return false;
    }
    unsafe { SendMessageW(hwnd_edit, EM_GETMODIFY, None, None).0 != 0 }
}

/// Clears or sets the modified state of the `"Edit"` control (`EM_SETMODIFY`).
pub fn set_edit_modified(hwnd_edit: HWND, modified: bool) {
    if hwnd_edit.is_invalid() {
        return;
    }
    unsafe {
        SendMessageW(
            hwnd_edit,
            EM_SETMODIFY,
            Some(WPARAM(usize::from(modified))),
            Some(LPARAM(0)),
        );
    }
}

/// Normalizes line endings to `\r\n` as required by the Win32 `Edit` control.
pub fn normalize_crlf(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            out.push('\r');
            if chars.peek() == Some(&'\n') {
                let _ = chars.next();
                out.push('\n');
            } else {
                out.push('\n');
            }
        } else if c == '\n' {
            out.push_str("\r\n");
        } else {
            out.push(c);
        }
    }
    out
}

#[allow(dead_code)]
fn _assert_pwstr_used(_p: PWSTR) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bom_detection_and_roundtrip() {
        let sample = "Hello\r\nNotepad Clone!";

        for enc in [
            TextEncoding::Utf8,
            TextEncoding::Utf8Bom,
            TextEncoding::Utf16Le,
            TextEncoding::Utf16Be,
        ] {
            let bytes = encode_notepad_bytes(sample, enc);
            let decoded = decode_notepad_bytes(&bytes);
            assert_eq!(decoded.encoding, enc);
            assert_eq!(decoded.text, sample);
            assert!(!decoded.is_log_file);
        }
    }

    #[test]
    fn test_null_sanitization_and_log_header() {
        let raw = b".LOG\r\nEntry\x00WithNull";
        let decoded = decode_notepad_bytes(raw);
        assert!(decoded.is_log_file);
        assert_eq!(decoded.text, ".LOG\r\nEntry WithNull");
    }

    #[test]
    fn test_win32_file_io_roundtrip() {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("notepad_win32_test_io.txt");

        let content = "Line 1\r\nLine 2 with UTF-8: café";
        save_file_win32(&file_path, content, TextEncoding::Utf8Bom).unwrap();

        let loaded = load_file_win32(&file_path).unwrap();
        assert_eq!(loaded.encoding, TextEncoding::Utf8Bom);
        assert_eq!(loaded.text, content);

        let _ = std::fs::remove_file(&file_path);
    }
}
