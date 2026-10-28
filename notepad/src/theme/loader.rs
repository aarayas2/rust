//! Parsers for `.sublime-color-scheme` (JSON) and `.tmTheme` (XML Property List) files.
//!
//! # Public Format Specifications
//! - **`.sublime-color-scheme` (JSON):**
//!   - `name`: String identifier of the color scheme.
//!   - `variables`: Key-value map of reusable color strings, referenced via `var(name)`.
//!   - `globals`: Editor-wide colors (`background`, `foreground`, `caret`, `selection`,
//!     `selection_border`, `line_highlight`, `gutter`, `gutter_foreground`,
//!     `gutter_foreground_highlight`, `brackets_foreground`).
//!   - `rules`: Array of `{ name, scope, foreground, background, font_style }` objects.
//! - **`.tmTheme` (TextMate XML Plist):**
//!   - Root `<dict>` containing `name` and `settings` (`<array>` of `<dict>`).
//!   - The first entry in `settings` with no `scope` key defines global editor settings.
//!   - Subsequent entries with a `scope` key define token styles (`foreground`, `background`, `fontStyle`).

use crate::theme::{ColorScheme, RgbColor, ScopeRule, Style, Theme};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Raw deserialized structure of a `.sublime-color-scheme` JSON file.
#[derive(Debug, Deserialize)]
struct RawSublimeColorScheme {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    variables: HashMap<String, String>,
    #[serde(default)]
    globals: HashMap<String, String>,
    #[serde(default)]
    rules: Vec<RawSublimeRule>,
}

/// Raw rule entry inside a `.sublime-color-scheme` JSON file.
#[derive(Debug, Deserialize)]
struct RawSublimeRule {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    foreground: Option<String>,
    #[serde(default)]
    background: Option<String>,
    #[serde(default)]
    font_style: Option<String>,
}

/// Resolves `var(variable_name)` references in a color string using the theme's `variables` map.
pub fn resolve_variable(raw: &str, variables: &HashMap<String, String>) -> String {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed.strip_prefix("var(").and_then(|s| s.strip_suffix(')')) {
        let key = inner.trim();
        if let Some(val) = variables.get(key) {
            return val.trim().to_string();
        }
    }
    trimmed.to_string()
}

/// Parses a hex color string (`#RRGGBB` or `#RRGGBBAA`) into an `RgbColor`.
/// If an alpha channel is present and `backdrop` is provided, alpha-blends over `backdrop`.
pub fn parse_hex_color(
    raw: &str,
    variables: &HashMap<String, String>,
    backdrop: Option<RgbColor>,
) -> Option<RgbColor> {
    let resolved = resolve_variable(raw, variables);
    let hex_digits = resolved.strip_prefix('#')?;
    match hex_digits.len() {
        6 => {
            let r = u8::from_str_radix(&hex_digits[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex_digits[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex_digits[4..6], 16).ok()?;
            Some(RgbColor::new(r, g, b))
        }
        8 => {
            let r = u8::from_str_radix(&hex_digits[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex_digits[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex_digits[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex_digits[6..8], 16).ok()?;
            let fg = RgbColor::new(r, g, b);
            if let Some(bg) = backdrop {
                Some(bg.blend(fg, a as u32, 255))
            } else {
                Some(fg)
            }
        }
        _ => None,
    }
}

/// Parses font style flags (`"bold"`, `"italic"`, `"underline"`) into `(bold, italic, underline)`.
fn parse_font_style_flags(font_style: Option<&str>) -> (bool, bool, bool) {
    let Some(raw) = font_style else {
        return (false, false, false);
    };
    let lower = raw.to_ascii_lowercase();
    let bold = lower.contains("bold");
    let italic = lower.contains("italic");
    let underline = lower.contains("underline");
    (bold, italic, underline)
}

/// Computes perceived luminance of an `RgbColor` to determine whether a palette is dark.
fn is_dark_background(bg: RgbColor) -> bool {
    let luma = (bg.r as u32 * 299 + bg.g as u32 * 587 + bg.b as u32 * 114) / 1000;
    luma < 128
}

/// Parses a `.sublime-color-scheme` JSON string into a [`ColorScheme`].
pub fn parse_sublime_color_scheme_str(
    json_src: &str,
    static_name: &'static str,
) -> Result<ColorScheme, String> {
    let raw: RawSublimeColorScheme = serde_json::from_str(json_src)
        .map_err(|e| format!("Failed to parse .sublime-color-scheme JSON: {e}"))?;

    let vars = &raw.variables;
    let background = raw
        .globals
        .get("background")
        .and_then(|s| parse_hex_color(s, vars, None))
        .unwrap_or(RgbColor::from_hex(0x212733));
    let foreground = raw
        .globals
        .get("foreground")
        .and_then(|s| parse_hex_color(s, vars, Some(background)))
        .unwrap_or(RgbColor::from_hex(0xD9E0EE));
    let is_dark = is_dark_background(background);

    let caret = raw
        .globals
        .get("caret")
        .and_then(|s| parse_hex_color(s, vars, Some(background)))
        .unwrap_or(RgbColor::from_hex(0xFFB454));
    let secondary_caret = raw
        .globals
        .get("block_caret")
        .and_then(|s| parse_hex_color(s, vars, Some(background)))
        .unwrap_or(RgbColor::from_hex(0x5CCFE6));
    let selection = raw
        .globals
        .get("selection")
        .and_then(|s| parse_hex_color(s, vars, Some(background)))
        .unwrap_or(RgbColor::from_hex(0x343F4C));
    let selection_border = raw
        .globals
        .get("selection_border")
        .and_then(|s| parse_hex_color(s, vars, Some(background)))
        .unwrap_or(RgbColor::from_hex(0x4B596B));
    let line_highlight = raw
        .globals
        .get("line_highlight")
        .and_then(|s| parse_hex_color(s, vars, Some(background)))
        .unwrap_or(RgbColor::from_hex(0x2B3340));
    let gutter = raw
        .globals
        .get("gutter")
        .and_then(|s| parse_hex_color(s, vars, Some(background)))
        .unwrap_or(background);
    let gutter_foreground = raw
        .globals
        .get("gutter_foreground")
        .and_then(|s| parse_hex_color(s, vars, Some(background)))
        .unwrap_or_else(|| foreground.blend(background, 55, 100));
    let gutter_foreground_highlight = raw
        .globals
        .get("gutter_foreground_highlight")
        .and_then(|s| parse_hex_color(s, vars, Some(background)))
        .unwrap_or(foreground);
    let brackets_foreground = raw
        .globals
        .get("brackets_foreground")
        .and_then(|s| parse_hex_color(s, vars, Some(background)))
        .unwrap_or(caret);

    let status_background = if is_dark {
        background.blend(RgbColor::new(0, 0, 0), 20, 100)
    } else {
        background.blend(RgbColor::new(0, 0, 0), 8, 100)
    };
    let status_foreground = foreground.blend(background, 25, 100);

    let theme = Theme {
        name: static_name,
        is_dark,
        background,
        foreground,
        caret,
        secondary_caret,
        selection,
        selection_border,
        line_highlight,
        gutter,
        gutter_foreground,
        gutter_foreground_highlight,
        status_background,
        status_foreground,
        brackets_foreground,
    };

    let mut rules = Vec::with_capacity(raw.rules.len());
    for r in raw.rules {
        if r.scope.trim().is_empty() {
            continue;
        }
        let fg = r
            .foreground
            .as_deref()
            .and_then(|s| parse_hex_color(s, vars, Some(background)))
            .unwrap_or(foreground);
        let bg = r
            .background
            .as_deref()
            .and_then(|s| parse_hex_color(s, vars, Some(background)));
        let (bold, italic, underline) = parse_font_style_flags(r.font_style.as_deref());
        rules.push(ScopeRule {
            name: r.name.unwrap_or_default(),
            scope_selectors: r
                .scope
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            style: Style {
                foreground: fg,
                background: bg,
                bold,
                italic,
                underline,
            },
        });
    }

    Ok(ColorScheme {
        name: raw.name.unwrap_or_else(|| static_name.to_string()),
        theme,
        rules,
    })
}

/// Parses a `.tmTheme` XML Property List string into a [`ColorScheme`].
pub fn parse_tmtheme_str(
    xml_src: &str,
    static_name: &'static str,
) -> Result<ColorScheme, String> {
    let val: plist::Value = plist::from_bytes(xml_src.as_bytes())
        .map_err(|e| format!("Failed to parse .tmTheme XML plist: {e}"))?;
    let root_dict = val
        .as_dictionary()
        .ok_or_else(|| "Root of .tmTheme is not a <dict>".to_string())?;

    let scheme_name = root_dict
        .get("name")
        .and_then(|v| v.as_string())
        .unwrap_or(static_name)
        .to_string();

    let settings_arr = root_dict
        .get("settings")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Missing `settings` <array> in .tmTheme".to_string())?;

    let empty_vars = HashMap::new();
    let mut globals_map: HashMap<String, String> = HashMap::new();
    let mut rules = Vec::new();

    for entry in settings_arr {
        let Some(entry_dict) = entry.as_dictionary() else {
            continue;
        };
        let scope_opt = entry_dict.get("scope").and_then(|v| v.as_string());
        let settings_dict = entry_dict.get("settings").and_then(|v| v.as_dictionary());

        if scope_opt.is_none() {
            if let Some(sdict) = settings_dict {
                for (k, v) in sdict {
                    if let Some(sv) = v.as_string() {
                        globals_map.insert(k.clone(), sv.to_string());
                    }
                }
            }
        } else if let (Some(scope_str), Some(sdict)) = (scope_opt, settings_dict) {
            let rule_name = entry_dict
                .get("name")
                .and_then(|v| v.as_string())
                .unwrap_or("")
                .to_string();
            let fg_str = sdict.get("foreground").and_then(|v| v.as_string());
            let bg_str = sdict.get("background").and_then(|v| v.as_string());
            let font_style_str = sdict.get("fontStyle").and_then(|v| v.as_string());

            let (bold, italic, underline) = parse_font_style_flags(font_style_str);
            let fg = fg_str
                .and_then(|s| parse_hex_color(s, &empty_vars, None))
                .unwrap_or(RgbColor::from_hex(0xF2F2EE));
            let bg = bg_str.and_then(|s| parse_hex_color(s, &empty_vars, None));

            rules.push(ScopeRule {
                name: rule_name,
                scope_selectors: scope_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                style: Style {
                    foreground: fg,
                    background: bg,
                    bold,
                    italic,
                    underline,
                },
            });
        }
    }

    let background = globals_map
        .get("background")
        .and_then(|s| parse_hex_color(s, &empty_vars, None))
        .unwrap_or(RgbColor::from_hex(0x22231E));
    let foreground = globals_map
        .get("foreground")
        .and_then(|s| parse_hex_color(s, &empty_vars, Some(background)))
        .unwrap_or(RgbColor::from_hex(0xF2F2EE));
    let is_dark = is_dark_background(background);

    let caret = globals_map
        .get("caret")
        .and_then(|s| parse_hex_color(s, &empty_vars, Some(background)))
        .unwrap_or(RgbColor::from_hex(0xFAF8F0));
    let selection = globals_map
        .get("selection")
        .and_then(|s| parse_hex_color(s, &empty_vars, Some(background)))
        .unwrap_or(RgbColor::from_hex(0x414339));
    let selection_border = globals_map
        .get("selectionBorder")
        .and_then(|s| parse_hex_color(s, &empty_vars, Some(background)))
        .unwrap_or(RgbColor::from_hex(0x55584A));
    let line_highlight = globals_map
        .get("lineHighlight")
        .and_then(|s| parse_hex_color(s, &empty_vars, Some(background)))
        .unwrap_or(RgbColor::from_hex(0x32342B));
    let gutter = globals_map
        .get("gutter")
        .and_then(|s| parse_hex_color(s, &empty_vars, Some(background)))
        .unwrap_or(background);
    let gutter_foreground = globals_map
        .get("gutterForeground")
        .and_then(|s| parse_hex_color(s, &empty_vars, Some(background)))
        .unwrap_or_else(|| foreground.blend(background, 55, 100));
    let gutter_foreground_highlight = globals_map
        .get("gutterForegroundHighlight")
        .and_then(|s| parse_hex_color(s, &empty_vars, Some(background)))
        .unwrap_or(foreground);
    let brackets_foreground = globals_map
        .get("bracketsForeground")
        .and_then(|s| parse_hex_color(s, &empty_vars, Some(background)))
        .unwrap_or(caret);

    let status_background = if is_dark {
        background.blend(RgbColor::new(0, 0, 0), 20, 100)
    } else {
        background.blend(RgbColor::new(0, 0, 0), 8, 100)
    };
    let status_foreground = foreground.blend(background, 25, 100);

    let theme = Theme {
        name: static_name,
        is_dark,
        background,
        foreground,
        caret,
        secondary_caret: RgbColor::from_hex(0x5AD4E6),
        selection,
        selection_border,
        line_highlight,
        gutter,
        gutter_foreground,
        gutter_foreground_highlight,
        status_background,
        status_foreground,
        brackets_foreground,
    };

    Ok(ColorScheme {
        name: scheme_name,
        theme,
        rules,
    })
}

/// Loads a [`ColorScheme`] from a `.sublime-color-scheme` or `.tmTheme` file on disk.
pub fn load_color_scheme_from_path(
    path: &Path,
    static_name: &'static str,
) -> Result<ColorScheme, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read theme file {}: {e}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "sublime-color-scheme" => parse_sublime_color_scheme_str(&content, static_name),
        "tmtheme" => parse_tmtheme_str(&content, static_name),
        _ => {
            if content.trim_start().starts_with('{') {
                parse_sublime_color_scheme_str(&content, static_name)
            } else {
                parse_tmtheme_str(&content, static_name)
            }
        }
    }
}
