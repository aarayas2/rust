//! Sublime Text-inspired Theming Engine (`ColorScheme`) and Line-Number Gutter Layout Calculator.
//!
//! # Reverse Engineering Notes (`C:\Program Files\Sublime Text\sublime_text.exe`)
//!
//! ## 1. Color Scheme Parsing & C++ Palette Structure (`0x140332980` & `0x140336200`)
//! - **`.tmTheme` Plist Settings Loader (`0x140332980` – `0x140333050`, base pointer `rbx`)** and
//!   **`.sublime-color-scheme` JSON `globals` Parser (`0x140336200` – `0x140336E50`, base pointer `r12`):**
//!   - String table references in `.rdata`:
//!     - `.sublime-color-scheme` (`0x1406BF56B`, `0x1406BF582`)
//!     - `.tmTheme` (`0x1406BF5ED`, `0x1406BF61E`)
//!     - `selection` (`0x1406BA7C4`), `selection_foreground` (`0x1406C170B`), `selection_border` (`0x1406B8D01`)
//!     - `line_highlight` (`0x1406B5271`), `caret` / `block_caret` (`0x1406B55FF`)
//!     - `gutter` (`0x1406B8296`), `gutter_foreground` (`0x1406C16D2`), `gutter_foreground_highlight` (`0x1406B5291`)
//!   - Resolved `ColorScheme` C++ struct (`r12`) stores active colors as 16-byte SIMD RGBA float vectors (`[f32; 4]`):
//!     - `[r12 + 0x308]`: `foreground`
//!     - `[r12 + 0x318]`: `background`
//!     - `[r12 + 0x3B8]`: `caret`
//!     - `[r12 + 0x3C8]`: `block_caret`
//!     - `[r12 + 0x3D8]`: `block_caret_border`
//!     - `[r12 + 0x400]`: `selection_border`
//!     - `[r12 + 0x410]`: `inactive_selection_border`
//!     - `[r12 + 0x424]`: `selection_border_width` (`u32`, clamped to `0..=10` at `0x140336A0B`)
//!     - `[r12 + 0x42C]` / `[r12 + 0x43C]`: `line_highlight` (alpha-blended over `background` `[r12 + 0x318]` at `0x14033650D`)
//!     - `[r12 + 0x46C]`: `gutter` background (`0x140336C96..0x140336D05`; falls back to `background` `[r12 + 0x318]` when omitted)
//!     - `[r12 + 0x47C]`: `gutter_foreground` (`0x140336D0E..0x140336D7D`; falls back to a blended mid-tone of `foreground` and `background`)
//!     - `[r12 + 0x48C]`: `gutter_foreground_highlight` (`0x140336D86..0x140336DFA`; falls back to `foreground` `[r12 + 0x308]`)
//!
//! ## 2. Line-Number Gutter Width & Viewport Separation (`0x140369ED4`, `0x14036B524`, `0x1403A1AC6`)
//! - **View Settings Binding (`0x14037E350` – `0x14037E750`, view struct `rsi`):**
//!   - `[rsi + 0x560]` (`f32`): `margin` (`0x1406BAFA9`, default `4.0` dip scaled by DPI via `0x1401CCD1C`)
//!   - `[rsi + 0x564]` (`bool`): `line_numbers` (`0x1406B675E`)
//!   - `[rsi + 0x565]` (`bool`): `relative_line_numbers` (`0x1406B6755`)
//!   - `[rsi + 0x566]` (`bool`): `gutter` (`0x1406B8296`)
//!   - `[rsi + 0x56B]` (`bool`): `highlight_gutter` (`0x1406B828C`)
//!   - `[rsi + 0x56C]` (`bool`): `highlight_line_number` (`0x1406B9140`)
//! - **Gutter Width Calculation (`0x140369ED4` -> `0x140365A14` -> `0x14036B524`):**
//!   - Queries total line count from the `TextBuffer` (`[rsi + 0x18]`), converts to `f32`, and computes
//!     `floorf(log10f(line_count))` (`0x14053D340` / `0x140560670`) to obtain the digit count `edi`.
//!   - Clamps the minimum digit count to `2` (`0x14036B590`: `cmp edi, 3; mov eax, 2; cmovge eax, edi`).
//!   - Measures the glyph advance width of ASCII `'0'` (`0x14036B586`: `mov edx, 0x30`, calling `0x14009A54C`
//!     which wraps GDI `GetTextExtentPointW` / `GetTextMetricsW`).
//!   - Computes `(digit_count * char_zero_width) + left_right_padding` (`mulss xmm1, xmm0` at `0x14036B59F`).
//! - **Viewport Separation (`0x1403A1AC6` & `0x140369FA9`):**
//!   - Subtracts `gutter_width` from the total client width (`subsd xmm2, xmm0; maxsd xmm2, 0.0`) before
//!     laying out the text buffer (`0x140365790`), and offsets the text viewport X coordinate by `gutter_width`
//!     (`addsd xmm7, xmm2` at `0x140369FA9`).

use windows::Win32::Foundation::COLORREF;

/// Represents an 8-bit per channel RGB color convertible to Win32 GDI `COLORREF` (`0x00BBGGRR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RgbColor {
    /// Creates a new `RgbColor` from `(r, g, b)` components.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Creates an `RgbColor` from a 24-bit hex literal `0xRRGGBB`.
    pub const fn from_hex(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xFF) as u8,
            g: ((hex >> 8) & 0xFF) as u8,
            b: (hex & 0xFF) as u8,
        }
    }

    /// Converts this `RgbColor` into a Win32 GDI `COLORREF` (`0x00BBGGRR`).
    pub const fn to_colorref(self) -> COLORREF {
        COLORREF((self.r as u32) | ((self.g as u32) << 8) | ((self.b as u32) << 16))
    }

    /// Linearly blends `self` toward `other` by `t_num / t_den` (mirrors Sublime Text's
    /// `gutter_foreground` / `line_highlight` fallback blending in `0x14033650D..0x140336D7D`).
    pub const fn blend(self, other: Self, t_num: u32, t_den: u32) -> Self {
        if t_den == 0 {
            return self;
        }
        let inv = t_den.saturating_sub(t_num);
        let r = ((self.r as u32 * inv + other.r as u32 * t_num) / t_den) as u8;
        let g = ((self.g as u32 * inv + other.g as u32 * t_num) / t_den) as u8;
        let b = ((self.b as u32 * inv + other.b as u32 * t_num) / t_den) as u8;
        Self { r, g, b }
    }
}

/// Built-in color scheme presets extracted from `Color Scheme - Default.sublime-package`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemePreset {
    /// Sublime Text's default dark theme (`Mariana.sublime-color-scheme`).
    MarianaDark,
    /// Sublime Text's classic dark theme (`Monokai.sublime-color-scheme`).
    MonokaiDark,
    /// Sublime Text's default light theme (`Breakers.sublime-color-scheme`).
    BreakersLight,
    /// Classic Windows Notepad high-contrast light appearance.
    ClassicNotepad,
}

/// Active editor and gutter color palette modeled after Sublime Text's C++ `ColorScheme`
/// structure (`r12` at `0x140336200` – `0x140336E50`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Human-readable color scheme name.
    pub name: &'static str,
    /// Whether this theme is a dark palette.
    pub is_dark: bool,
    /// Main text editor background color (`[r12 + 0x318]`).
    pub background: RgbColor,
    /// Main text editor foreground text color (`[r12 + 0x308]`).
    pub foreground: RgbColor,
    /// Primary and multi-cursor caret color (`[r12 + 0x3B8]`).
    pub caret: RgbColor,
    /// Secondary multi-cursor caret color (`[r12 + 0x3C8]`).
    pub secondary_caret: RgbColor,
    /// Selection fill color (`selection`).
    pub selection: RgbColor,
    /// Selection & gutter divider border color (`[r12 + 0x400]`).
    pub selection_border: RgbColor,
    /// Active cursor line highlight background (`[r12 + 0x42C]`).
    pub line_highlight: RgbColor,
    /// Left line-number gutter background color (`[r12 + 0x46C]`).
    pub gutter: RgbColor,
    /// Inactive line number text color in the gutter (`[r12 + 0x47C]`).
    pub gutter_foreground: RgbColor,
    /// Highlighted line number text color for lines containing an active cursor (`[r12 + 0x48C]`).
    pub gutter_foreground_highlight: RgbColor,
    /// Status bar background color.
    pub status_background: RgbColor,
    /// Status bar foreground text color.
    pub status_foreground: RgbColor,
}

impl Theme {
    /// Returns the `Theme` palette corresponding to a `ThemePreset`.
    pub const fn from_preset(preset: ThemePreset) -> Self {
        match preset {
            ThemePreset::MarianaDark => Self::mariana_dark(),
            ThemePreset::MonokaiDark => Self::monokai_dark(),
            ThemePreset::BreakersLight => Self::breakers_light(),
            ThemePreset::ClassicNotepad => Self::classic_notepad(),
        }
    }

    /// **Mariana (Sublime Default Dark)** — extracted from `Mariana.sublime-color-scheme`:
    /// - `background`: `hsl(210, 15%, 22%)` -> `#303841`
    /// - `foreground`: `hsl(219, 28%, 88%)` -> `#D8DEE9`
    /// - `caret`: `hsl(32, 93%, 66%)` -> `#F9AE58`
    /// - `line_highlight` / `selection`: `hsla(210, 13%, 40%, 0.7)` -> `#4E5A65`
    /// - `selection_border`: `hsl(210, 13%, 45%)` -> `#647382`
    /// - `gutter`: `#303841` (defaults to `background` per `0x140336CF7`)
    /// - `gutter_foreground`: `#788594` (blended mid-tone of `foreground` & `background` per `0x140336D50`)
    /// - `gutter_foreground_highlight`: `#D8DEE9` (active line highlight per `0x140336DEB`)
    pub const fn mariana_dark() -> Self {
        let bg = RgbColor::from_hex(0x303841);
        let fg = RgbColor::from_hex(0xD8DEE9);
        Self {
            name: "Mariana (Sublime Dark)",
            is_dark: true,
            background: bg,
            foreground: fg,
            caret: RgbColor::from_hex(0xF9AE58),
            secondary_caret: RgbColor::from_hex(0x5FB3B3),
            selection: RgbColor::from_hex(0x4E5A65),
            selection_border: RgbColor::from_hex(0x647382),
            line_highlight: RgbColor::from_hex(0x3C4651),
            gutter: bg,
            gutter_foreground: fg.blend(bg, 55, 100),
            gutter_foreground_highlight: fg,
            status_background: RgbColor::from_hex(0x262D34),
            status_foreground: RgbColor::from_hex(0xA6B2C0),
        }
    }

    /// **Monokai (Sublime Classic Dark)** — extracted from `Monokai.sublime-color-scheme`:
    /// - `background`: `hsl(70, 8%, 15%)` -> `#272822`
    /// - `foreground`: `hsl(60, 30%, 96%)` -> `#F8F8F2`
    /// - `caret`: `#F8F8F0`
    /// - `line_highlight`: `hsl(55, 11%, 22%)` -> `#3E3D32`
    /// - `selection`: `hsla(55, 8%, 31%, 0.7)` -> `#49483E`
    pub const fn monokai_dark() -> Self {
        let bg = RgbColor::from_hex(0x272822);
        let fg = RgbColor::from_hex(0xF8F8F2);
        Self {
            name: "Monokai (Sublime Dark)",
            is_dark: true,
            background: bg,
            foreground: fg,
            caret: RgbColor::from_hex(0xF8F8F0),
            secondary_caret: RgbColor::from_hex(0x66D9EF),
            selection: RgbColor::from_hex(0x49483E),
            selection_border: RgbColor::from_hex(0x5A594E),
            line_highlight: RgbColor::from_hex(0x3E3D32),
            gutter: bg,
            gutter_foreground: RgbColor::from_hex(0x90908A),
            gutter_foreground_highlight: fg,
            status_background: RgbColor::from_hex(0x1E1F1C),
            status_foreground: RgbColor::from_hex(0xCFCFC2),
        }
    }

    /// **Breakers (Sublime Default Light)** — extracted from `Breakers.sublime-color-scheme`:
    /// - `background`: `hsl(180, 9%, 99%)` -> `#FCFEFE`
    /// - `foreground`: `hsl(0, 0%, 20%)` -> `#333333`
    /// - `caret`: `hsl(180, 36%, 54%)` -> `#5FB3B3`
    /// - `line_highlight` / `selection`: `hsla(204, 10%, 86%, 0.7)` -> `#D7DCE0`
    pub const fn breakers_light() -> Self {
        let bg = RgbColor::from_hex(0xFCFEFE);
        let fg = RgbColor::from_hex(0x333333);
        Self {
            name: "Breakers (Sublime Light)",
            is_dark: false,
            background: bg,
            foreground: fg,
            caret: RgbColor::from_hex(0x5FB3B3),
            secondary_caret: RgbColor::from_hex(0x2F7E7E),
            selection: RgbColor::from_hex(0xD7DCE0),
            selection_border: RgbColor::from_hex(0xC2C8CE),
            line_highlight: RgbColor::from_hex(0xEEF2F5),
            gutter: RgbColor::from_hex(0xF4F7F8),
            gutter_foreground: RgbColor::from_hex(0x8C969E),
            gutter_foreground_highlight: fg,
            status_background: RgbColor::from_hex(0xEAEFF2),
            status_foreground: RgbColor::from_hex(0x444B52),
        }
    }

    /// **Classic Notepad Light** — clean white background with subtle gray line-number gutter.
    pub const fn classic_notepad() -> Self {
        Self {
            name: "Classic Notepad (Light)",
            is_dark: false,
            background: RgbColor::from_hex(0xFFFFFF),
            foreground: RgbColor::from_hex(0x000000),
            caret: RgbColor::from_hex(0x005AB4),
            secondary_caret: RgbColor::from_hex(0x1E8CEB),
            selection: RgbColor::from_hex(0xCCE8FF),
            selection_border: RgbColor::from_hex(0xD9D9D9),
            line_highlight: RgbColor::from_hex(0xF3F8FE),
            gutter: RgbColor::from_hex(0xF5F5F5),
            gutter_foreground: RgbColor::from_hex(0x767676),
            gutter_foreground_highlight: RgbColor::from_hex(0x000000),
            status_background: RgbColor::from_hex(0xF0F0F0),
            status_foreground: RgbColor::from_hex(0x1E1E1E),
        }
    }
}

/// Line-number gutter geometry calculator modeled directly after Sublime Text's
/// `0x140369ED4` (`get_gutter_width`) and `0x14036B524` (`calculate_line_number_column_width`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GutterLayout {
    /// Whether the left line-number gutter is enabled (`[rsi + 0x564]` / `[rsi + 0x566]`).
    pub visible: bool,
    /// Number of base-10 digits reserved for line numbers (clamped to `>= 2` per `0x14036B590`).
    pub digit_count: usize,
    /// Measured pixel advance width of ASCII `'0'` (`0x30` at `0x14036B586`).
    pub char_zero_width: i32,
    /// Horizontal margin on each side of the line-number column (`[rsi + 0x560]`).
    pub horizontal_margin: i32,
    /// Computed total pixel width of the left gutter (`0` when `visible == false`).
    pub total_width: i32,
}

impl GutterLayout {
    /// Default horizontal padding (in pixels) applied on both left and right sides of the
    /// line-number digits in the gutter.
    pub const DEFAULT_MARGIN_PX: i32 = 10;

    /// Computes the number of base-10 digits required to display `total_lines`, clamped to a
    /// minimum of `2` digits (`cmp edi, 3; mov eax, 2; cmovge eax, edi` at `0x14036B590` in `sublime_text.exe`).
    pub fn digit_count(total_lines: usize) -> usize {
        let clamped_lines = total_lines.max(1);
        let digits = (clamped_lines as f64).log10().floor() as usize + 1;
        digits.max(2)
    }

    /// Computes the complete `GutterLayout` for the given `total_lines`, `char_zero_width`,
    /// `horizontal_margin`, and `visible` state.
    pub fn compute(
        total_lines: usize,
        char_zero_width: i32,
        horizontal_margin: i32,
        visible: bool,
    ) -> Self {
        let digit_count = Self::digit_count(total_lines);
        let safe_char_w = char_zero_width.max(6);
        let safe_margin = horizontal_margin.max(4);
        let total_width = if visible {
            (digit_count as i32 * safe_char_w) + (safe_margin * 2)
        } else {
            0
        };
        Self {
            visible,
            digit_count,
            char_zero_width: safe_char_w,
            horizontal_margin: safe_margin,
            total_width,
        }
    }

    /// Splits a client area of `client_width` into `(gutter_width, edit_x, edit_width)`
    /// mirroring Sublime Text's viewport split at `0x1403A1AC6` and `0x140369FA9`.
    pub fn split_viewport(&self, client_width: i32) -> (i32, i32, i32) {
        let gutter_w = self.total_width.min(client_width.max(0));
        let edit_x = gutter_w;
        let edit_w = (client_width - gutter_w).max(0);
        (gutter_w, edit_x, edit_w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgb_color_to_colorref_bgr_order() {
        let mariana_bg = RgbColor::from_hex(0x303841);
        assert_eq!(mariana_bg.r, 0x30);
        assert_eq!(mariana_bg.g, 0x38);
        assert_eq!(mariana_bg.b, 0x41);
        // Win32 COLORREF is 0x00BBGGRR
        assert_eq!(mariana_bg.to_colorref().0, 0x0041_3830);
    }

    #[test]
    fn test_sublime_digit_count_clamping_and_log10() {
        // Minimum 2 digits for 1..=99 lines (matches 0x14036B590: cmp edi, 3; mov eax, 2; cmovge eax, edi)
        assert_eq!(GutterLayout::digit_count(0), 2);
        assert_eq!(GutterLayout::digit_count(1), 2);
        assert_eq!(GutterLayout::digit_count(9), 2);
        assert_eq!(GutterLayout::digit_count(99), 2);
        // 3 digits for 100..=999 lines
        assert_eq!(GutterLayout::digit_count(100), 3);
        assert_eq!(GutterLayout::digit_count(999), 3);
        // 4 digits for 1000..=9999 lines
        assert_eq!(GutterLayout::digit_count(1000), 4);
        assert_eq!(GutterLayout::digit_count(9999), 4);
        // 5 digits for 10000+ lines
        assert_eq!(GutterLayout::digit_count(10_000), 5);
    }

    #[test]
    fn test_gutter_layout_width_and_viewport_split() {
        let layout_2 = GutterLayout::compute(42, 8, 10, true);
        assert_eq!(layout_2.digit_count, 2);
        assert_eq!(layout_2.total_width, 2 * 8 + 20); // 36px
        assert_eq!(layout_2.split_viewport(800), (36, 36, 764));

        let layout_4 = GutterLayout::compute(2500, 8, 10, true);
        assert_eq!(layout_4.digit_count, 4);
        assert_eq!(layout_4.total_width, 4 * 8 + 20); // 52px
        assert_eq!(layout_4.split_viewport(800), (52, 52, 748));

        let layout_hidden = GutterLayout::compute(2500, 8, 10, false);
        assert_eq!(layout_hidden.total_width, 0);
        assert_eq!(layout_hidden.split_viewport(800), (0, 0, 800));
    }

    #[test]
    fn test_theme_presets_have_distinct_gutter_and_highlight_colors() {
        for preset in [
            ThemePreset::MarianaDark,
            ThemePreset::MonokaiDark,
            ThemePreset::BreakersLight,
            ThemePreset::ClassicNotepad,
        ] {
            let theme = Theme::from_preset(preset);
            assert_ne!(
                theme.background, theme.foreground,
                "Theme {} must have contrasting background and foreground",
                theme.name
            );
            assert_ne!(
                theme.gutter, theme.gutter_foreground,
                "Theme {} must have contrasting gutter background and foreground",
                theme.name
            );
            assert_ne!(
                theme.gutter_foreground, theme.gutter_foreground_highlight,
                "Theme {} must distinguish active line number highlight from inactive line numbers",
                theme.name
            );
        }
    }
}
