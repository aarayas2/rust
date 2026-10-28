//! Color Theme Engine (`Theme`, `Style`, `ColorScheme`, `ScopeRule`, `ThemeRegistry`)
//! and Line-Number Gutter Layout Calculator (`GutterLayout`).
//!
//! Supports both `.sublime-color-scheme` (JSON with `variables`, `globals`, and `rules`)
//! and `.tmTheme` (TextMate XML Plist) files loaded from `assets/themes/`.

pub mod loader;
pub mod manager;

pub use loader::{
    load_color_scheme_from_path, parse_hex_color, parse_sublime_color_scheme_str,
    parse_tmtheme_str, resolve_variable,
};
pub use manager::{ThemeRegistry, GLOBAL_THEME_REGISTRY};

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

    /// Linearly blends `self` toward `other` by `t_num / t_den`.
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

/// Built-in color scheme presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemePreset {
    /// Original Mariana Dark clone (`assets/themes/Mariana.sublime-color-scheme`).
    MarianaDark,
    /// Original Monokai Dark clone (`assets/themes/Monokai.tmTheme`).
    MonokaiDark,
    /// Clean Daylight Light theme (`BreakersLight`).
    BreakersLight,
    /// Classic Windows Notepad high-contrast light appearance.
    ClassicNotepad,
}

/// Resolved visual style for a syntax token span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub foreground: RgbColor,
    pub background: Option<RgbColor>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// A scope selector rule mapping hierarchical scopes (e.g. `keyword.control`, `comment.line`,
/// `string.quoted`) to a [`Style`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeRule {
    pub name: String,
    pub scope_selectors: Vec<String>,
    pub style: Style,
}

impl ScopeRule {
    /// Computes the specificity score of this rule against a dotted `token_scope`
    /// (e.g. `"keyword.control.rust"` or `"comment.line.double-slash.rust"`).
    /// Returns `0` when no selector matches.
    pub fn match_score(&self, token_scope: &str) -> usize {
        if token_scope.is_empty() {
            return 0;
        }
        let mut best = 0usize;
        // A token scope string may contain space-separated scope segments from a stack,
        // or a single dotted leaf scope. Check against every segment.
        for candidate in token_scope.split_whitespace() {
            for selector in &self.scope_selectors {
                let sel = selector.trim();
                if sel.is_empty() {
                    continue;
                }
                if candidate == sel {
                    best = best.max(sel.len() + 100);
                } else if candidate.starts_with(sel)
                    && candidate.as_bytes().get(sel.len()) == Some(&b'.')
                {
                    best = best.max(sel.len() + 50);
                }
            }
        }
        best
    }
}

/// Complete parsed color scheme combining editor-wide [`Theme`] globals with
/// token-level [`ScopeRule`]s loaded from `.sublime-color-scheme` or `.tmTheme`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorScheme {
    pub name: String,
    pub theme: Theme,
    pub rules: Vec<ScopeRule>,
}

impl ColorScheme {
    /// Creates a `ColorScheme` from a [`Theme`] using default dark or light scope rules.
    pub fn from_theme(theme: Theme) -> Self {
        let rules = if theme.is_dark {
            manager::default_dark_scope_rules()
        } else {
            manager::default_light_scope_rules()
        };
        Self {
            name: theme.name.to_string(),
            theme,
            rules,
        }
    }

    /// Resolves the highest-specificity [`Style`] for a syntax `scope` string
    /// (such as `"comment.line"`, `"keyword.control"`, `"string.quoted"`, etc.).
    pub fn style_for_scope(&self, scope: &str) -> Style {
        let mut best_score = 0usize;
        let mut best_style = Style {
            foreground: self.theme.foreground,
            background: None,
            bold: false,
            italic: false,
            underline: false,
        };

        for rule in &self.rules {
            let score = rule.match_score(scope);
            if score > best_score {
                best_score = score;
                best_style = rule.style;
            }
        }
        best_style
    }
}

/// Active editor and gutter color palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Human-readable color scheme name.
    pub name: &'static str,
    /// Whether this theme is a dark palette.
    pub is_dark: bool,
    /// Main text editor background color.
    pub background: RgbColor,
    /// Main text editor foreground text color.
    pub foreground: RgbColor,
    /// Primary and multi-cursor caret color.
    pub caret: RgbColor,
    /// Secondary multi-cursor caret color.
    pub secondary_caret: RgbColor,
    /// Selection fill color.
    pub selection: RgbColor,
    /// Selection & gutter divider border color.
    pub selection_border: RgbColor,
    /// Active cursor line highlight background.
    pub line_highlight: RgbColor,
    /// Left line-number gutter background color.
    pub gutter: RgbColor,
    /// Inactive line number text color in the gutter.
    pub gutter_foreground: RgbColor,
    /// Highlighted line number text color for lines containing an active cursor.
    pub gutter_foreground_highlight: RgbColor,
    /// Status bar background color.
    pub status_background: RgbColor,
    /// Status bar foreground text color.
    pub status_foreground: RgbColor,
    /// Matching bracket underline/highlight color.
    pub brackets_foreground: RgbColor,
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

    /// **Mariana Clone (Original Dark)** — matches `assets/themes/Mariana.sublime-color-scheme`:
    /// - `background`: `#2B333C`
    /// - `foreground`: `#DCE3EC`
    /// - `caret`: `#F6A64C`
    /// - `line_highlight`: `#36404B`
    /// - `selection`: `#485460`
    /// - `selection_border`: `#607080`
    pub const fn mariana_dark() -> Self {
        let bg = RgbColor::from_hex(0x2B333C);
        let fg = RgbColor::from_hex(0xDCE3EC);
        Self {
            name: "Mariana (Sublime Dark)",
            is_dark: true,
            background: bg,
            foreground: fg,
            caret: RgbColor::from_hex(0xF6A64C),
            secondary_caret: RgbColor::from_hex(0x58B8B8),
            selection: RgbColor::from_hex(0x485460),
            selection_border: RgbColor::from_hex(0x607080),
            line_highlight: RgbColor::from_hex(0x36404B),
            gutter: bg,
            gutter_foreground: RgbColor::from_hex(0x7E8C9C),
            gutter_foreground_highlight: fg,
            status_background: RgbColor::from_hex(0x212830),
            status_foreground: RgbColor::from_hex(0xA4B2C2),
            brackets_foreground: RgbColor::from_hex(0xF6A64C),
        }
    }

    /// **Monokai Clone (Original Dark)** — matches `assets/themes/Monokai.tmTheme`:
    /// - `background`: `#22231E`
    /// - `foreground`: `#F2F2EE`
    /// - `caret`: `#FAF8F0`
    /// - `line_highlight`: `#32342B`
    /// - `selection`: `#414339`
    pub const fn monokai_dark() -> Self {
        let bg = RgbColor::from_hex(0x22231E);
        let fg = RgbColor::from_hex(0xF2F2EE);
        Self {
            name: "Monokai (Sublime Dark)",
            is_dark: true,
            background: bg,
            foreground: fg,
            caret: RgbColor::from_hex(0xFAF8F0),
            secondary_caret: RgbColor::from_hex(0x5AD4E6),
            selection: RgbColor::from_hex(0x414339),
            selection_border: RgbColor::from_hex(0x55584A),
            line_highlight: RgbColor::from_hex(0x32342B),
            gutter: bg,
            gutter_foreground: RgbColor::from_hex(0x888A7E),
            gutter_foreground_highlight: fg,
            status_background: RgbColor::from_hex(0x1B1C18),
            status_foreground: RgbColor::from_hex(0xCFCFC2),
            brackets_foreground: RgbColor::from_hex(0xFD9353),
        }
    }

    /// **Breakers Clone (Original Light)** — clean daylight theme.
    pub const fn breakers_light() -> Self {
        let bg = RgbColor::from_hex(0xFAFCFD);
        let fg = RgbColor::from_hex(0x2E343B);
        Self {
            name: "Breakers (Sublime Light)",
            is_dark: false,
            background: bg,
            foreground: fg,
            caret: RgbColor::from_hex(0x3A9D9D),
            secondary_caret: RgbColor::from_hex(0x267373),
            selection: RgbColor::from_hex(0xD5DFE5),
            selection_border: RgbColor::from_hex(0xB8C4CC),
            line_highlight: RgbColor::from_hex(0xECF1F4),
            gutter: RgbColor::from_hex(0xF2F6F8),
            gutter_foreground: RgbColor::from_hex(0x86929C),
            gutter_foreground_highlight: fg,
            status_background: RgbColor::from_hex(0xE6ECEF),
            status_foreground: RgbColor::from_hex(0x3F4750),
            brackets_foreground: RgbColor::from_hex(0xD96826),
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
            brackets_foreground: RgbColor::from_hex(0x005AB4),
        }
    }

    /// Resolves the [`Style`] for a syntax `scope` string using [`GLOBAL_THEME_REGISTRY`].
    pub fn style_for_scope(&self, scope: &str) -> Style {
        if let Some(scheme) = GLOBAL_THEME_REGISTRY.scheme_by_name(self.name) {
            scheme.style_for_scope(scope)
        } else {
            ColorScheme::from_theme(*self).style_for_scope(scope)
        }
    }
}

/// Line-number gutter geometry calculator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GutterLayout {
    /// Whether the left line-number gutter is enabled.
    pub visible: bool,
    /// Number of base-10 digits reserved for line numbers (clamped to `>= 2`).
    pub digit_count: usize,
    /// Measured pixel advance width of ASCII `'0'`.
    pub char_zero_width: i32,
    /// Horizontal margin on each side of the line-number column.
    pub horizontal_margin: i32,
    /// Computed total pixel width of the left gutter (`0` when `visible == false`).
    pub total_width: i32,
}

impl GutterLayout {
    /// Default horizontal padding (in pixels) applied on both left and right sides of the
    /// line-number digits in the gutter.
    pub const DEFAULT_MARGIN_PX: i32 = 10;

    /// Computes the number of base-10 digits required to display `total_lines`, clamped to a
    /// minimum of `2` digits.
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

    /// Splits a client area of `client_width` into `(gutter_width, edit_x, edit_width)`.
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
        let mariana_bg = RgbColor::from_hex(0x212733);
        assert_eq!(mariana_bg.r, 0x21);
        assert_eq!(mariana_bg.g, 0x27);
        assert_eq!(mariana_bg.b, 0x33);
        // Win32 COLORREF is 0x00BBGGRR
        assert_eq!(mariana_bg.to_colorref().0, 0x0033_2721);
    }

    #[test]
    fn test_sublime_digit_count_clamping_and_log10() {
        assert_eq!(GutterLayout::digit_count(0), 2);
        assert_eq!(GutterLayout::digit_count(1), 2);
        assert_eq!(GutterLayout::digit_count(9), 2);
        assert_eq!(GutterLayout::digit_count(99), 2);
        assert_eq!(GutterLayout::digit_count(100), 3);
        assert_eq!(GutterLayout::digit_count(999), 3);
        assert_eq!(GutterLayout::digit_count(1000), 4);
        assert_eq!(GutterLayout::digit_count(9999), 4);
        assert_eq!(GutterLayout::digit_count(10_000), 5);
    }

    #[test]
    fn test_gutter_layout_width_and_viewport_split() {
        let layout_2 = GutterLayout::compute(42, 8, 10, true);
        assert_eq!(layout_2.digit_count, 2);
        assert_eq!(layout_2.total_width, 2 * 8 + 20);
        assert_eq!(layout_2.split_viewport(800), (36, 36, 764));

        let layout_4 = GutterLayout::compute(2500, 8, 10, true);
        assert_eq!(layout_4.digit_count, 4);
        assert_eq!(layout_4.total_width, 4 * 8 + 20);
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

    #[test]
    fn test_load_mariana_sublime_color_scheme_and_monokai_tmtheme() {
        let mut reg = ThemeRegistry::new_default();
        let mariana = reg.scheme_for_preset(ThemePreset::MarianaDark);
        assert_eq!(mariana.theme.background, RgbColor::from_hex(0x2B333C));
        let kw_style = mariana.style_for_scope("keyword.control.rust");
        let comment_style = mariana.style_for_scope("comment.line.double-slash.rust");
        let str_style = mariana.style_for_scope("string.quoted.double.rust");
        assert_ne!(kw_style.foreground, mariana.theme.foreground);
        assert_ne!(comment_style.foreground, kw_style.foreground);
        assert!(comment_style.italic);
        assert_ne!(str_style.foreground, kw_style.foreground);

        // Live switch to Monokai (.tmTheme)
        let monokai = reg.set_active_preset(ThemePreset::MonokaiDark);
        assert_eq!(reg.active_preset(), ThemePreset::MonokaiDark);
        assert_eq!(monokai.theme.background, RgbColor::from_hex(0x22231E));
        let monokai_kw = monokai.style_for_scope("keyword.control.python");
        let monokai_str = monokai.style_for_scope("string.quoted.double.python");
        assert_ne!(monokai_kw.foreground, monokai.theme.foreground);
        assert_ne!(monokai_str.foreground, monokai_kw.foreground);
    }
}
