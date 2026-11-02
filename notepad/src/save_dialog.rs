use std::os::windows::ffi::OsStrExt;
//! Reverse-engineered File Open & Save Dialog implementations matching Win32 Notepad.
//!
//! # Reverse Engineering Notes
//!
//! In classic Notepad (`C:\Windows\System32\notepad.exe`):
//! - **Save Dialog Function:** `0x14000D2E8` (`GetSaveFileNameW` at `0x14000D45D`, IAT `0x140039020`).
//! - **Open Dialog Function:** `0x14000D078` (`GetOpenFileNameW` at `0x14000D1E7`, IAT `0x140039050`).
//! - **Error Handling:** `CommDlgExtendedError` at `0x14000D4AC` and `0x14000DB6A`.
//!   - Returns `0` when the user cancels the dialog (`HRESULT_FROM_WIN32(ERROR_CANCELLED)` = `0x800704C7`).
//!   - Returns non-zero error code on Common Dialog failure (`E_FAIL` = `0x80004005`).

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_ENABLESIZING, OFN_EXPLORER,
    OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    OPEN_FILENAME_FLAGS,
};

/// Buffer size matching Notepad's allocation: 260 characters (`MAX_PATH`, `mov edi, 0x104` at `0x14000D321`).
pub const MAX_PATH_LEN: usize = 260;

/// Default Notepad file filter:
/// `"Text Documents (*.txt)\0*.txt\0All Files (*.*)\0*.*\0\0"`
pub const DEFAULT_NOTEPAD_FILTER: &str = "Text Documents (*.txt)\0*.txt\0All Files (*.*)\0*.*\0\0";

/// Open dialog file filter containing common code and text files:
pub const OPEN_DIALOG_FILTER: &str = concat!(
    "All Files (*.*)\0*.*\0",
    "ASP (*.vbs)\0*.vbs\0",
    "ActionScript 2.0 (*.as)\0*.as\0",
    "AppleScript (*.applescript)\0*.applescript\0",
    "Bash (*.sh; *.bash)\0*.sh;*.bash\0",
    "Batch File (*.bat; *.cmd)\0*.bat;*.cmd\0",
    "BibTeX (*.bib)\0*.bib\0",
    "C (*.c; *.h)\0*.c;*.h\0",
    "C# (*.cs; *.csx)\0*.cs;*.csx\0",
    "C++ (*.cpp; *.cc; *.h; *.hpp; *.ino)\0*.cpp;*.cc;*.h;*.hpp;*.ino\0",
    "CSS (*.css)\0*.css\0",
    "Cabal (*.cabal)\0*.cabal\0",
    "Cassandra (*.cql)\0*.cql\0",
    "Clojure (*.clj; *.cljs; *.edn)\0*.clj;*.cljs;*.edn\0",
    "ClojureScript (*.cljs; *.cljc)\0*.cljs;*.cljc\0",
    "D (*.d; *.di)\0*.d;*.di\0",
    "DTD (*.dtd)\0*.dtd\0",
    "Dart (*.dart)\0*.dart\0",
    "Diff (*.diff; *.patch)\0*.diff;*.patch\0",
    "Erlang (*.erl; *.hrl)\0*.erl;*.hrl\0",
    "Git Attributes (.gitattributes)\0.gitattributes\0",
    "Git Config (.gitconfig)\0.gitconfig\0",
    "Git Ignore (.gitignore)\0.gitignore\0",
    "Go (*.go)\0*.go\0",
    "HTML (*.html; *.htm)\0*.html;*.htm\0",
    "JSON (*.json)\0*.json\0",
    "Java (*.java)\0*.java\0",
    "JavaScript (*.js; *.mjs; *.cjs)\0*.js;*.mjs;*.cjs\0",
    "Kotlin (*.kt; *.kts)\0*.kt;*.kts\0",
    "Lua (*.lua)\0*.lua\0",
    "Markdown (*.md; *.markdown)\0*.md;*.markdown\0",
    "PHP (*.php)\0*.php\0",
    "Perl (*.pl; *.pm)\0*.pl;*.pm\0",
    "PowerShell (*.ps1; *.psm1; *.psd1)\0*.ps1;*.psm1;*.psd1\0",
    "Python (*.py; *.pyw; *.pyi)\0*.py;*.pyw;*.pyi\0",
    "R (*.r; *.R; *.Rmd)\0*.r;*.R;*.Rmd\0",
    "Ruby (*.rb; *.erb)\0*.rb;*.erb\0",
    "Rust (*.rs)\0*.rs\0",
    "SQL (*.sql)\0*.sql\0",
    "Shell (*.sh; *.zsh)\0*.sh;*.zsh\0",
    "Swift (*.swift)\0*.swift\0",
    "TOML (*.toml)\0*.toml\0",
    "Text (*.txt; *.log; *.csv)\0*.txt;*.log;*.csv\0",
    "TypeScript (*.ts; *.tsx)\0*.ts;*.tsx\0",
    "XML (*.xml; *.xsd; *.xsl)\0*.xml;*.xsd;*.xsl\0",
    "YAML (*.yaml; *.yml)\0*.yaml;*.yml\0",
    "\0"
);

/// Dialog error representation.
#[derive(Debug, PartialEq, Eq)]
pub enum SaveDialogError {
    CommonDialogError(u32),
    InvalidEncoding,
}

impl fmt::Display for SaveDialogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommonDialogError(code) => {
                write!(
                    f,
                    "Dialog internal error (CommDlgExtendedError: 0x{:08X})",
                    code
                )
            }
            Self::InvalidEncoding => write!(f, "Invalid UTF-16 encoding in selected path"),
        }
    }
}

impl Error for SaveDialogError {}

/// Builder and runner for a safe Win32 Save File Dialog (`0x14000D2E8`).
pub struct SaveFileDialog<'a> {
    owner: HWND,
    default_extension: &'a str,
    filter: &'a str,
    initial_file: Option<&'a Path>,
    flags: OPEN_FILENAME_FLAGS,
}

impl<'a> SaveFileDialog<'a> {
    /// Creates a new `SaveFileDialog` associated with an owner window.
    pub fn new(owner: HWND) -> Self {
        Self {
            owner,
            default_extension: "txt",
            filter: DEFAULT_NOTEPAD_FILTER,
            initial_file: None,
            // Replicating Notepad's flags (`0x888866` at `0x14000D401`):
            flags: OFN_OVERWRITEPROMPT
                | OFN_PATHMUSTEXIST
                | OFN_HIDEREADONLY
                | OFN_EXPLORER
                | OFN_ENABLESIZING,
        }
    }

    /// Sets the default extension to append if not provided by the user.
    pub fn default_extension(mut self, ext: &'a str) -> Self {
        self.default_extension = ext;
        self
    }

    /// Sets the initial filename to populate in the edit box.
    pub fn initial_file(mut self, path: &'a Path) -> Self {
        self.initial_file = Some(path);
        self
    }

    /// Displays the modal Save dialog and returns the chosen file path, or `None` if canceled.
    pub fn show(self) -> Result<Option<PathBuf>, SaveDialogError> {
        run_common_file_dialog(
            self.owner,
            self.default_extension,
            self.filter,
            self.initial_file,
            self.flags,
            true,
        )
    }
}

/// Builder and runner for a safe Win32 Open File Dialog (`0x14000D078`).
pub struct OpenFileDialog<'a> {
    owner: HWND,
    default_extension: &'a str,
    filter: &'a str,
    initial_file: Option<&'a Path>,
    flags: OPEN_FILENAME_FLAGS,
}

impl<'a> OpenFileDialog<'a> {
    /// Creates a new `OpenFileDialog` associated with an owner window.
    pub fn new(owner: HWND) -> Self {
        Self {
            owner,
            default_extension: "txt",
            filter: OPEN_DIALOG_FILTER,
            initial_file: None,
            flags: OFN_FILEMUSTEXIST
                | OFN_PATHMUSTEXIST
                | OFN_HIDEREADONLY
                | OFN_EXPLORER
                | OFN_ENABLESIZING,
        }
    }

    /// Sets the default extension.
    pub fn default_extension(mut self, ext: &'a str) -> Self {
        self.default_extension = ext;
        self
    }

    /// Displays the modal Open dialog and returns the chosen file path, or `None` if canceled.
    pub fn show(self) -> Result<Option<PathBuf>, SaveDialogError> {
        run_common_file_dialog(
            self.owner,
            self.default_extension,
            self.filter,
            self.initial_file,
            self.flags,
            false,
        )
    }
}

fn run_common_file_dialog(
    owner: HWND,
    default_extension: &str,
    filter: &str,
    initial_file: Option<&Path>,
    flags: OPEN_FILENAME_FLAGS,
    is_save: bool,
) -> Result<Option<PathBuf>, SaveDialogError> {
    let mut file_buf = [0u16; MAX_PATH_LEN];

    if let Some(init_path) = initial_file {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = init_path.as_os_str().encode_wide().collect();
        let copy_len = wide.len().min(file_buf.len() - 1);
        file_buf[..copy_len].copy_from_slice(&wide[..copy_len]);
    }

    let filter_wide: Vec<u16> = filter.encode_utf16().chain(std::iter::once(0)).collect();
    let ext_wide: Vec<u16> = default_extension
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter_wide.as_ptr()),
        nFilterIndex: 1,
        lpstrFile: PWSTR(file_buf.as_mut_ptr()),
        nMaxFile: file_buf.len() as u32,
        lpstrDefExt: PCWSTR(ext_wide.as_ptr()),
        Flags: flags,
        ..Default::default()
    };

    let success = unsafe {
        if is_save {
            GetSaveFileNameW(&mut ofn)
        } else {
            GetOpenFileNameW(&mut ofn)
        }
    };

    if success.as_bool() {
        let len = file_buf
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(file_buf.len());
        let path_str =
            String::from_utf16(&file_buf[..len]).map_err(|_| SaveDialogError::InvalidEncoding)?;
        Ok(Some(PathBuf::from(path_str)))
    } else {
        let error_code = unsafe { CommDlgExtendedError() };
        if error_code.0 == 0 {
            Ok(None)
        } else {
            Err(SaveDialogError::CommonDialogError(error_code.0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_dialog_builder_defaults() {
        let dialog = SaveFileDialog::new(HWND::default());
        assert_eq!(dialog.default_extension, "txt");
        assert_eq!(dialog.filter, DEFAULT_NOTEPAD_FILTER);
        assert!(dialog.initial_file.is_none());
    }

    #[test]
    fn test_open_dialog_builder_defaults() {
        let dialog = OpenFileDialog::new(HWND::default());
        assert_eq!(dialog.default_extension, "txt");
        assert_eq!(dialog.filter, OPEN_DIALOG_FILTER);
    }

    #[test]
    fn test_filter_format() {
        assert!(DEFAULT_NOTEPAD_FILTER.ends_with("\0\0"));
    }

    #[test]
    fn test_open_dialog_filter_format() {
        assert!(OPEN_DIALOG_FILTER.ends_with("\0\0"));
        assert!(OPEN_DIALOG_FILTER.starts_with("All Files (*.*)\0*.*\0"));
    }
}
