//! Sublime Text-inspired Editor Font State & Piecewise Font Scaling Engine.
//!
//! # Reverse Engineering Notes (`C:\Program Files\Sublime Text\sublime_text.exe`)
//!
//! ## 1. Menu Structure & Settings (`Default.sublime-package`)
//! - **`Main.sublime-menu` (lines 956–966, REA Evidence `ev_a41fc07c9a3202a41df846533863563ad743d1eaa16a3b379512388ca5031c09`):**
//!   - Under `Preferences` (`"id": "preferences"`), the `Font` submenu defines:
//!     - `{ "command": "choose_font", "caption": "Choose…" }`
//!     - `{ "command": "increase_font_size", "caption": "Larger" }`
//!     - `{ "command": "decrease_font_size", "caption": "Smaller" }`
//!     - `{ "caption": "-" }`
//!     - `{ "command": "reset_font_size", "caption": "Reset Size" }`
//! - **`Preferences (Windows).sublime-settings` & `Preferences.sublime-settings`:**
//!   - `"font_face": "Consolas"` (`0x1406C132C`)
//!   - `"font_size": 11` (`0x1406BDEF7`, base default `10`)
//!   - `"default_font_size": 11` (`0x1406BDEB9`, base default `10`)
//! - **`Default (Windows).sublime-keymap` (lines 772–780):**
//!   - `ctrl++`, `ctrl+=`, `ctrl+keypad_plus` -> `increase_font_size`
//!   - `ctrl+-`, `ctrl+keypad_minus` -> `decrease_font_size`
//!
//! ## 2. Native C++ Font Scaling Routines in `sublime_text.exe`
//! - **`increase_font_size` ("Larger", `0x1400FEBF6` -> `"increase_font_size"` at `0x1406BDEDB`; body at `0x1400FEBFE` – `0x1400FECC9`):**
//!   - Loads `"Preferences.sublime-settings"` (`0x1406B6E60` at `0x1400FEC10`).
//!   - Reads `"font_size"` (`0x1406BDEF7` at `0x1400FEC32`, fallback `10` at `0x1400FEC29`) into `eax`.
//!   - Applies a piecewise step curve (`0x1400FEC56` – `0x1400FEC7F`):
//!     - `cmp eax, 0x23` (`35`): if `eax > 35` (`>= 36`), clamps `eax` to `<= 0x7C` (`124`) via
//!       `cmp eax, 0x7C; mov ecx, 0x7C; cmovb ecx, eax` and adds `+4` (`add ecx, 4` at `0x1400FEC66`),
//!       capping at `128` (`0x80`).
//!     - `cmp eax, 0x18` (`24`): if `24 <= eax <= 35`, adds `+2` (`add eax, 2` at `0x1400FEC74`).
//!     - Otherwise (`eax < 24`), adds `+1` (`inc eax` at `0x1400FEC79`).
//!   - Persists `"font_size"` (`call 0x140067A4C` at `0x1400FEC98`) and flushes `"Preferences.sublime-settings"`.
//! - **`decrease_font_size` ("Smaller", `0x1400FECCA` -> `"decrease_font_size"` at `0x1406BDEEE`; body at `0x1400FECD2` – `0x1400FED9D`):**
//!   - Reads `"font_size"` (`0x1406BDEF7`, fallback `10`) into `eax`.
//!   - Applies the inverse piecewise step curve (`0x1400FED2A` – `0x1400FED53`):
//!     - `cmp eax, 0x28` (`40`): if `eax >= 40`, subtracts `4` (`add eax, -4` at `0x1400FED2F`).
//!     - `cmp eax, 0x1A` (`26`): if `26 <= eax < 40`, subtracts `2` (`add eax, -2` at `0x1400FED39`).
//!     - Otherwise (`eax < 26`), clamps `eax` to `>= 9` (`cmp eax, 0x0A; mov ecx, 9; cmovge ecx, eax`)
//!       and decrements by `1` (`dec ecx` at `0x1400FED4D`), enforcing a minimum of `8` pt.
//! - **`reset_font_size` ("Reset Size", `0x1400FED9E` -> `"reset_font_size"` at `0x1406BDECB`; body at `0x1400FEDA6` – `0x1400FEE5F`):**
//!   - Reads `"default_font_size"` (`0x1406BDEB9` at `0x1400FEDEA`, fallback `10` at `0x1400FEDDF`)
//!     and resets `"font_size"` (`0x1406BDEF7` at `0x1400FEE0F`) to `default_font_size`.
//! - **View Font Clamping (`0x14037D705` – `0x14037D7E2`):**
//!   - Clamps view `font_size` (`f32`) between `2.0f` (`0x1405AA7B8` -> `0x40000000` at `0x14037D7C7`)
//!     and `128.0f` (`0x1405C1A98` -> `0x43000000` at `0x14037D7DA`).
//!
//! ## 3. Dynamic Window Title Formatting in `sublime_text.exe` (`0x140047900` – `0x140047F35` & `0x1401E20FB`)
//! - Reads `"show_full_path"` (`0x1406BCD34` at `0x140047A60`, default `true` on Windows).
//! - Formats active document path or `"untitled"` (`0x1406C223C`), checks `view->is_dirty()` (`call [rax+0x38]`
//!   at `0x140047CEF`), and appends `" •"` (`0x1406B2517`, UTF-8 bytes `20 E2 80 A2`) when modified.
//! - In `0x1401E20FB`, appends `" - "` (`0x1406DAD4E`) and `"Sublime Text"` (`0x1406B479A`) and invokes
//!   Win32 `SetWindowTextW` (`call qword ptr [0x1406E1278]` at `0x1401E22C4`).

use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    CreateFontW, CLEARTYPE_QUALITY, DEFAULT_CHARSET, FF_MODERN, FIXED_PITCH, FW_NORMAL, HFONT,
    OUT_TT_PRECIS, CLIP_DEFAULT_PRECIS,
};

/// Default editor font face on Windows (`Preferences (Windows).sublime-settings` line 2).
pub const DEFAULT_WINDOWS_FONT_FACE: &str = "Consolas";

/// Default editor font point size on Windows (`Preferences (Windows).sublime-settings` line 3).
pub const DEFAULT_WINDOWS_FONT_SIZE: i32 = 11;

/// Minimum point size enforced by `decrease_font_size` (`0x1400FED42`: `mov ecx, 9; dec ecx` -> `8`).
pub const MIN_MENU_FONT_SIZE: i32 = 8;

/// Maximum point size enforced by `increase_font_size` (`0x1400FEC5E`: `mov ecx, 0x7C; add ecx, 4` -> `128`)
/// and View font clamping (`0x1405C1A98 = 128.0f` at `0x14037D7D1`).
pub const MAX_FONT_SIZE: i32 = 128;

/// Absolute minimum view font size in Sublime Text's view clamping (`0x1405AA7B8 = 2.0f` at `0x14037D7BA`).
pub const MIN_VIEW_FONT_SIZE: i32 = 2;

/// Tracks the active editor font family, current point size, default reset point size,
/// and scaling operations modeled after Sublime Text (`0x1400FEBF6` – `0x1400FEE5F`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontState {
    /// Active monospace font family name (`"font_face"`, e.g. `"Consolas"`).
    pub font_face: String,
    /// Current editor font size in points (`"font_size"`).
    pub font_size: i32,
    /// Default font size in points restored by `Reset Size` (`"default_font_size"`).
    pub default_font_size: i32,
}

impl Default for FontState {
    fn default() -> Self {
        Self::new(DEFAULT_WINDOWS_FONT_FACE, DEFAULT_WINDOWS_FONT_SIZE)
    }
}

impl FontState {
    /// Creates a new `FontState` with the specified `font_face` and `default_font_size`.
    pub fn new(font_face: impl Into<String>, default_font_size: i32) -> Self {
        let clamped_default = default_font_size.clamp(MIN_VIEW_FONT_SIZE, MAX_FONT_SIZE);
        Self {
            font_face: font_face.into(),
            font_size: clamped_default,
            default_font_size: clamped_default,
        }
    }

    /// Increases the font size following Sublime Text's exact piecewise step curve
    /// in `increase_font_size` (`0x1400FEC56` – `0x1400FEC7F`):
    /// - `< 24`: `+1` (`inc eax`)
    /// - `24..=35`: `+2` (`add eax, 2`)
    /// - `>= 36`: `min(size, 124) + 4` (`cmp eax, 0x7C; mov ecx, 0x7C; cmovb ecx, eax; add ecx, 4`)
    pub fn increase_size(&mut self) -> i32 {
        let eax = self.font_size;
        let next = if eax > 0x23 {
            // 0x1400FEC5B: cmp eax, 0x7C; mov ecx, 0x7C; cmovb ecx, eax; add ecx, 4
            let ecx = eax.min(0x7C);
            ecx + 4
        } else if eax >= 0x18 {
            // 0x1400FEC6F: cmp eax, 0x18; jl ...; add eax, 2
            eax + 2
        } else {
            // 0x1400FEC79: inc eax
            eax + 1
        };
        self.font_size = next.clamp(MIN_VIEW_FONT_SIZE, MAX_FONT_SIZE);
        self.font_size
    }

    /// Decreases the font size following Sublime Text's exact piecewise step curve
    /// in `decrease_font_size` (`0x1400FED2A` – `0x1400FED53`):
    /// - `>= 40`: `-4` (`add eax, -4`)
    /// - `26..=39`: `-2` (`add eax, -2`)
    /// - `< 26`: `max(size, 9) - 1` (`cmp eax, 0x0A; mov ecx, 9; cmovge ecx, eax; dec ecx` -> floor `8`)
    pub fn decrease_size(&mut self) -> i32 {
        let eax = self.font_size;
        let next = if eax >= 0x28 {
            // 0x1400FED2A: cmp eax, 0x28; add eax, -4
            eax - 4
        } else if eax >= 0x1A {
            // 0x1400FED34: cmp eax, 0x1A; add eax, -2
            eax - 2
        } else {
            // 0x1400FED42: cmp eax, 0x0A; mov ecx, 9; cmovge ecx, eax; dec ecx
            let ecx = if eax >= 0x0A { eax } else { 9 };
            ecx - 1
        };
        self.font_size = next.clamp(MIN_MENU_FONT_SIZE, MAX_FONT_SIZE);
        self.font_size
    }

    /// Resets `font_size` back to `default_font_size` (`reset_font_size` at `0x1400FEDA6` – `0x1400FEE5F`).
    pub fn reset_size(&mut self) -> i32 {
        self.font_size = self.default_font_size;
        self.font_size
    }

    /// Sets a custom font size, clamped to Sublime Text's view bounds `[2, 128]` (`0x14037D7B6` – `0x14037D7E2`).
    pub fn set_size(&mut self, size: i32) -> i32 {
        self.font_size = size.clamp(MIN_VIEW_FONT_SIZE, MAX_FONT_SIZE);
        self.font_size
    }

    /// Updates the active `font_face` name (`"font_face"` at `0x1400FEB6C`).
    pub fn set_face(&mut self, face: impl Into<String>) {
        let face_str = face.into();
        if !face_str.trim().is_empty() {
            self.font_face = face_str;
        }
    }

    /// Returns the current zoom/scale percentage relative to `default_font_size` (e.g., `100` for `11pt`).
    pub fn scale_percent(&self) -> u32 {
        let base = self.default_font_size.max(1) as f64;
        ((self.font_size as f64 / base) * 100.0).round() as u32
    }

    /// Computes the negative GDI logical character height (`-MulDiv(point_size, dpi, 72)`) for `CreateFontW`.
    pub fn logical_height_for_dpi(&self, dpi: i32) -> i32 {
        let safe_dpi = if dpi > 0 { dpi } else { 96 };
        -((self.font_size * safe_dpi + 36) / 72)
    }

    /// Creates a Win32 GDI `HFONT` matching the current `font_face` and `font_size` at `dpi`.
    pub unsafe fn create_gdi_font(&self, dpi: i32) -> HFONT {
        let height = self.logical_height_for_dpi(dpi);
        let wide_face: Vec<u16> = self
            .font_face
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        CreateFontW(
            height,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_TT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (FIXED_PITCH.0 | FF_MODERN.0) as u32,
            PCWSTR(wide_face.as_ptr()),
        )
    }
}

/// Formats the main window title string following Sublime Text's title builder
/// (`0x140047900` – `0x140047F35` and `0x1401E20FB` – `0x1401E22E3`):
/// - Uses full path when `show_full_path` is true (`0x1406BCD34`), or filename otherwise.
/// - Uses `"untitled"` (or first-line preview if unsaved and non-empty, per `set_unsaved_view_name.py`).
/// - Appends `" •"` (`0x1406B2517`, space + `U+2022` bullet) when `is_dirty` is true (`0x140047CF6`).
/// - Appends `" - "` (`0x1406DAD4E`) and the application title (`"Sublime Notepad"`).
pub fn format_sublime_window_title(
    current_file: Option<&Path>,
    buffer_preview: &str,
    is_dirty: bool,
    show_full_path: bool,
    font_state: &FontState,
) -> String {
    let doc_title = if let Some(path) = current_file {
        if show_full_path {
            path.display().to_string()
        } else {
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string())
        }
    } else {
        // Mirror set_unsaved_view_name.py: use trimmed first line (up to 50 chars) if non-empty, else "untitled"
        let first_line = buffer_preview
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(50)
            .collect::<String>();
        if first_line.is_empty() {
            "untitled".to_string()
        } else {
            first_line
        }
    };

    // 0x140047CF6: append " •" (0x1406B2517: 20 E2 80 A2) when view->is_dirty() is non-zero
    let dirty_suffix = if is_dirty { " \u{2022}" } else { "" };

    // Include font size indicator when scaled away from default_font_size
    let font_badge = if font_state.font_size != font_state.default_font_size {
        format!(" [{}pt]", font_state.font_size)
    } else {
        String::new()
    };

    // 0x1401E217A: append " - " + application name
    format!("{}{}{} - Notepad", doc_title, dirty_suffix, font_badge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_sublime_increase_font_size_piecewise_curve() {
        let mut font = FontState::new("Consolas", 11);
        assert_eq!(font.font_size, 11);

        // < 24 increments by +1 (0x1400FEC79: inc eax)
        assert_eq!(font.increase_size(), 12);
        font.set_size(23);
        assert_eq!(font.increase_size(), 24);

        // 24..=35 increments by +2 (0x1400FEC74: add eax, 2)
        assert_eq!(font.increase_size(), 26);
        font.set_size(35);
        assert_eq!(font.increase_size(), 37);

        // >= 36 increments by +4 and caps at 128 (0x1400FEC5E: mov ecx, 0x7C; cmovb ecx, eax; add ecx, 4)
        font.set_size(36);
        assert_eq!(font.increase_size(), 40);
        font.set_size(124);
        assert_eq!(font.increase_size(), 128);
        assert_eq!(font.increase_size(), 128);
    }

    #[test]
    fn test_sublime_decrease_font_size_piecewise_curve_and_reset() {
        let mut font = FontState::new("Consolas", 11);

        // >= 40 decrements by -4 (0x1400FED2F: add eax, -4)
        font.set_size(44);
        assert_eq!(font.decrease_size(), 40);
        assert_eq!(font.decrease_size(), 36);

        // 26..=39 decrements by -2 (0x1400FED39: add eax, -2)
        assert_eq!(font.decrease_size(), 34);
        font.set_size(26);
        assert_eq!(font.decrease_size(), 24);

        // < 26 decrements by -1 down to floor 8 (0x1400FED42: mov ecx, 9; cmovge ecx, eax; dec ecx)
        assert_eq!(font.decrease_size(), 23);
        font.set_size(9);
        assert_eq!(font.decrease_size(), 8);
        assert_eq!(font.decrease_size(), 8);

        // Reset Size restores default_font_size (11) (0x1400FEDA6)
        assert_eq!(font.reset_size(), 11);
        assert_eq!(font.scale_percent(), 100);
    }

    #[test]
    fn test_sublime_window_title_formatting() {
        let mut font = FontState::new("Consolas", 11);
        // 1. Empty untitled clean buffer
        assert_eq!(
            format_sublime_window_title(None, "", false, true, &font),
            "untitled - Notepad"
        );
        // 2. Unsaved buffer with first line text & dirty bullet (" •")
        assert_eq!(
            format_sublime_window_title(None, "fn main() {\r\n}", true, true, &font),
            "fn main() { \u{2022} - Notepad"
        );
        // 3. Saved file with full path vs filename only and scaled font
        let path = PathBuf::from(r"D:\github\rust\notepad\src\main.rs");
        font.increase_size(); // 12pt
        assert_eq!(
            format_sublime_window_title(Some(&path), "", false, true, &font),
            r"D:\github\rust\notepad\src\main.rs [12pt] - Notepad"
        );
        assert_eq!(
            format_sublime_window_title(Some(&path), "", true, false, &font),
            "main.rs \u{2022} [12pt] - Notepad"
        );
    }
}
