//! Theme Manager and Registry (`ThemeRegistry`) for loading and live-switching color schemes.

use crate::theme::loader::{
    load_color_scheme_from_path, parse_sublime_color_scheme_str, parse_tmtheme_str,
};
use crate::theme::{ColorScheme, RgbColor, ScopeRule, Style, Theme, ThemePreset};
use once_cell::sync::Lazy;
use std::fs;
use std::path::Path;

/// Embedded fallback `.sublime-color-scheme` for Mariana Clone.
const EMBEDDED_MARIANA_SCHEME: &str =
    include_str!("../../assets/themes/Mariana.sublime-color-scheme");

/// Embedded fallback `.tmTheme` for Monokai Clone.
const EMBEDDED_MONOKAI_TMTHEME: &str = include_str!("../../assets/themes/Monokai.tmTheme");

/// Global `ThemeRegistry` singleton initialized from `assets/themes/`.
pub static GLOBAL_THEME_REGISTRY: Lazy<ThemeRegistry> = Lazy::new(ThemeRegistry::new_default);

/// Registry managing all available [`ColorScheme`]s (loaded from `.tmTheme` and
/// `.sublime-color-scheme` files in `assets/themes/`, plus built-in light themes)
/// and supporting live switching.
#[derive(Debug, Clone)]
pub struct ThemeRegistry {
    schemes: Vec<(ThemePreset, ColorScheme)>,
    custom_schemes: Vec<ColorScheme>,
    active_preset: ThemePreset,
}

impl ThemeRegistry {
    /// Creates a `ThemeRegistry` loading from `assets/themes` on disk with embedded fallbacks.
    pub fn new_default() -> Self {
        let mut registry = Self {
            schemes: Vec::new(),
            custom_schemes: Vec::new(),
            active_preset: ThemePreset::MarianaDark,
        };
        registry.reload_from_dir(Path::new("assets/themes"));
        registry
    }

    /// Loads or reloads `.sublime-color-scheme` and `.tmTheme` files from `dir`.
    pub fn reload_from_dir(&mut self, dir: &Path) {
        self.schemes.clear();
        self.custom_schemes.clear();

        let mariana_path = dir.join("Mariana.sublime-color-scheme");
        let mariana_scheme = load_color_scheme_from_path(&mariana_path, "Mariana (Sublime Dark)")
            .or_else(|_| {
                parse_sublime_color_scheme_str(EMBEDDED_MARIANA_SCHEME, "Mariana (Sublime Dark)")
            })
            .unwrap_or_else(|_| ColorScheme::from_theme(Theme::mariana_dark()));

        let monokai_path = dir.join("Monokai.tmTheme");
        let monokai_scheme = load_color_scheme_from_path(&monokai_path, "Monokai (Sublime Dark)")
            .or_else(|_| parse_tmtheme_str(EMBEDDED_MONOKAI_TMTHEME, "Monokai (Sublime Dark)"))
            .unwrap_or_else(|_| ColorScheme::from_theme(Theme::monokai_dark()));

        let breakers_scheme = ColorScheme::from_theme(Theme::breakers_light());
        let classic_scheme = ColorScheme::from_theme(Theme::classic_notepad());

        self.schemes.push((ThemePreset::MarianaDark, mariana_scheme));
        self.schemes.push((ThemePreset::MonokaiDark, monokai_scheme));
        self.schemes
            .push((ThemePreset::BreakersLight, breakers_scheme));
        self.schemes
            .push((ThemePreset::ClassicNotepad, classic_scheme));

        // Discover any additional user-added .sublime-color-scheme or .tmTheme files in `dir`.
        if dir.is_dir() {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let file_name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");
                    if file_name.eq_ignore_ascii_case("Mariana.sublime-color-scheme")
                        || file_name.eq_ignore_ascii_case("Monokai.tmTheme")
                    {
                        continue;
                    }
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if ext == "sublime-color-scheme" || ext == "tmtheme" {
                        if let Ok(scheme) = load_color_scheme_from_path(&path, "Custom Theme") {
                            self.custom_schemes.push(scheme);
                        }
                    }
                }
            }
        }
    }

    /// Returns the [`ColorScheme`] associated with a [`ThemePreset`].
    pub fn scheme_for_preset(&self, preset: ThemePreset) -> ColorScheme {
        self.schemes
            .iter()
            .find(|(p, _)| *p == preset)
            .map(|(_, s)| s.clone())
            .unwrap_or_else(|| ColorScheme::from_theme(Theme::from_preset(preset)))
    }

    /// Looks up a [`ColorScheme`] by name (case-insensitive substring or exact match).
    pub fn scheme_by_name(&self, query: &str) -> Option<ColorScheme> {
        let q = query.trim().to_ascii_lowercase();
        for (_, scheme) in &self.schemes {
            if scheme.name.to_ascii_lowercase().contains(&q)
                || scheme.theme.name.to_ascii_lowercase().contains(&q)
            {
                return Some(scheme.clone());
            }
        }
        for scheme in &self.custom_schemes {
            if scheme.name.to_ascii_lowercase().contains(&q) {
                return Some(scheme.clone());
            }
        }
        None
    }

    /// Returns the currently active [`ThemePreset`].
    pub fn active_preset(&self) -> ThemePreset {
        self.active_preset
    }

    /// Switches the active [`ThemePreset`] and returns the newly activated [`ColorScheme`].
    pub fn set_active_preset(&mut self, preset: ThemePreset) -> ColorScheme {
        self.active_preset = preset;
        self.scheme_for_preset(preset)
    }

    /// Returns the currently active [`ColorScheme`].
    pub fn active_scheme(&self) -> ColorScheme {
        self.scheme_for_preset(self.active_preset)
    }

    /// Lists all loaded [`ColorScheme`]s (both preset-backed and custom).
    pub fn all_schemes(&self) -> Vec<ColorScheme> {
        let mut all: Vec<ColorScheme> = self.schemes.iter().map(|(_, s)| s.clone()).collect();
        all.extend(self.custom_schemes.iter().cloned());
        all
    }
}

/// Builds default light-mode scope rules for `BreakersLight` and `ClassicNotepad`.
pub(crate) fn default_light_scope_rules() -> Vec<ScopeRule> {
    vec![
        ScopeRule {
            name: "Comments".to_string(),
            scope_selectors: vec!["comment".to_string(), "punctuation.definition.comment".to_string()],
            style: Style {
                foreground: RgbColor::from_hex(0x6E7781),
                background: None,
                bold: false,
                italic: true,
                underline: false,
            },
        },
        ScopeRule {
            name: "Keywords".to_string(),
            scope_selectors: vec![
                "keyword".to_string(),
                "keyword.control".to_string(),
                "keyword.declaration".to_string(),
                "storage.modifier".to_string(),
            ],
            style: Style {
                foreground: RgbColor::from_hex(0xAF00DB),
                background: None,
                bold: true,
                italic: false,
                underline: false,
            },
        },
        ScopeRule {
            name: "Operators".to_string(),
            scope_selectors: vec!["keyword.operator".to_string()],
            style: Style {
                foreground: RgbColor::from_hex(0x0550AE),
                background: None,
                bold: false,
                italic: false,
                underline: false,
            },
        },
        ScopeRule {
            name: "Storage Types".to_string(),
            scope_selectors: vec!["storage.type".to_string(), "support.type".to_string()],
            style: Style {
                foreground: RgbColor::from_hex(0x0969DA),
                background: None,
                bold: false,
                italic: true,
                underline: false,
            },
        },
        ScopeRule {
            name: "Functions".to_string(),
            scope_selectors: vec![
                "entity.name.function".to_string(),
                "support.function".to_string(),
            ],
            style: Style {
                foreground: RgbColor::from_hex(0x8250DF),
                background: None,
                bold: false,
                italic: false,
                underline: false,
            },
        },
        ScopeRule {
            name: "Types & Classes".to_string(),
            scope_selectors: vec![
                "entity.name.type".to_string(),
                "entity.name.struct".to_string(),
                "entity.name.enum".to_string(),
                "entity.name.trait".to_string(),
                "entity.name.class".to_string(),
            ],
            style: Style {
                foreground: RgbColor::from_hex(0x953800),
                background: None,
                bold: true,
                italic: false,
                underline: false,
            },
        },
        ScopeRule {
            name: "Strings".to_string(),
            scope_selectors: vec!["string".to_string(), "string.quoted".to_string()],
            style: Style {
                foreground: RgbColor::from_hex(0x0A3069),
                background: None,
                bold: false,
                italic: false,
                underline: false,
            },
        },
        ScopeRule {
            name: "Numbers & Constants".to_string(),
            scope_selectors: vec![
                "constant.numeric".to_string(),
                "constant.language".to_string(),
                "constant.character".to_string(),
            ],
            style: Style {
                foreground: RgbColor::from_hex(0x0550AE),
                background: None,
                bold: false,
                italic: false,
                underline: false,
            },
        },
    ]
}

/// Builds default dark-mode scope rules as a fallback when constructing a `ColorScheme` directly from `Theme`.
pub(crate) fn default_dark_scope_rules() -> Vec<ScopeRule> {
    vec![
        ScopeRule {
            name: "Comments".to_string(),
            scope_selectors: vec!["comment".to_string(), "punctuation.definition.comment".to_string()],
            style: Style {
                foreground: RgbColor::from_hex(0x738294),
                background: None,
                bold: false,
                italic: true,
                underline: false,
            },
        },
        ScopeRule {
            name: "Keywords".to_string(),
            scope_selectors: vec![
                "keyword".to_string(),
                "keyword.control".to_string(),
                "keyword.declaration".to_string(),
                "storage.modifier".to_string(),
            ],
            style: Style {
                foreground: RgbColor::from_hex(0xC792EA),
                background: None,
                bold: true,
                italic: false,
                underline: false,
            },
        },
        ScopeRule {
            name: "Operators".to_string(),
            scope_selectors: vec!["keyword.operator".to_string()],
            style: Style {
                foreground: RgbColor::from_hex(0xF07178),
                background: None,
                bold: false,
                italic: false,
                underline: false,
            },
        },
        ScopeRule {
            name: "Storage Types".to_string(),
            scope_selectors: vec!["storage.type".to_string(), "support.type".to_string()],
            style: Style {
                foreground: RgbColor::from_hex(0x5CCFE6),
                background: None,
                bold: false,
                italic: true,
                underline: false,
            },
        },
        ScopeRule {
            name: "Functions".to_string(),
            scope_selectors: vec![
                "entity.name.function".to_string(),
                "support.function".to_string(),
            ],
            style: Style {
                foreground: RgbColor::from_hex(0x73D0FF),
                background: None,
                bold: false,
                italic: false,
                underline: false,
            },
        },
        ScopeRule {
            name: "Types & Classes".to_string(),
            scope_selectors: vec![
                "entity.name.type".to_string(),
                "entity.name.struct".to_string(),
                "entity.name.enum".to_string(),
                "entity.name.trait".to_string(),
                "entity.name.class".to_string(),
            ],
            style: Style {
                foreground: RgbColor::from_hex(0xFFD580),
                background: None,
                bold: false,
                italic: false,
                underline: false,
            },
        },
        ScopeRule {
            name: "Strings".to_string(),
            scope_selectors: vec!["string".to_string(), "string.quoted".to_string()],
            style: Style {
                foreground: RgbColor::from_hex(0xBAE67E),
                background: None,
                bold: false,
                italic: false,
                underline: false,
            },
        },
        ScopeRule {
            name: "Numbers & Constants".to_string(),
            scope_selectors: vec![
                "constant.numeric".to_string(),
                "constant.language".to_string(),
                "constant.character".to_string(),
            ],
            style: Style {
                foreground: RgbColor::from_hex(0xFF9E64),
                background: None,
                bold: false,
                italic: false,
                underline: false,
            },
        },
    ]
}
