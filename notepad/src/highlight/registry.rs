//! Extensible Language Syntax Definition Registry (`SyntaxDefinition`, `SyntaxRegistry`)
//! and File-Type Detection Engine (extension + first-line shebang/header match).

use once_cell::sync::Lazy;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use syntect::parsing::{
    ParseState, SyntaxDefinition as SyntectSyntaxDef, SyntaxReference, SyntaxSet, SyntaxSetBuilder,
};

const EMBEDDED_RUST_SYNTAX: &str = include_str!("../../assets/syntaxes/Rust.sublime-syntax");
const EMBEDDED_JS_SYNTAX: &str = include_str!("../../assets/syntaxes/JavaScript.sublime-syntax");
const EMBEDDED_PYTHON_SYNTAX: &str = include_str!("../../assets/syntaxes/Python.sublime-syntax");

/// Global default `SyntaxRegistry` initialized lazily once via `once_cell`.
pub static GLOBAL_SYNTAX_REGISTRY: Lazy<SyntaxRegistry> = Lazy::new(SyntaxRegistry::load_default);

/// Trait defining a syntax grammar entry in the language registry.
pub trait SyntaxDefinition: Send + Sync {
    /// Human-readable display name (e.g., `"Rust"`, `"JavaScript"`, `"Python"`, `"Plain Text"`).
    fn name(&self) -> &str;
    /// Lowercase file extensions mapped to this syntax (e.g., `["rs"]`, `["js", "mjs", "cjs"]`).
    fn file_extensions(&self) -> &[String];
    /// Root TextMate/Sublime scope selector (e.g., `"source.rust"`, `"source.js"`, `"source.python"`).
    fn scope(&self) -> &str;
    /// Optional regular expression matched against line 1 of a file (shebang / modeline).
    fn first_line_match(&self) -> Option<&str>;
    /// Origin / loader descriptor (e.g., `"assets/syntaxes/Rust.sublime-syntax"` or `"builtin"`).
    fn loader(&self) -> &str;
}

/// Concrete registered syntax definition metadata paired with the compiled `SyntaxSet`.
#[derive(Debug, Clone)]
pub struct RegisteredSyntax {
    pub name: String,
    pub file_extensions: Vec<String>,
    pub scope: String,
    pub first_line_match: Option<String>,
    pub loader: String,
}

impl SyntaxDefinition for RegisteredSyntax {
    fn name(&self) -> &str {
        &self.name
    }

    fn file_extensions(&self) -> &[String] {
        &self.file_extensions
    }

    fn scope(&self) -> &str {
        &self.scope
    }

    fn first_line_match(&self) -> Option<&str> {
        self.first_line_match.as_deref()
    }

    fn loader(&self) -> &str {
        &self.loader
    }
}

/// Deserialized YAML header of a `.sublime-syntax` file via `serde_yaml`.
#[derive(Debug, Deserialize)]
struct SublimeSyntaxYamlHeader {
    name: Option<String>,
    #[serde(default)]
    file_extensions: Vec<String>,
    first_line_match: Option<String>,
    scope: Option<String>,
}

/// Registry holding compiled `syntect::parsing::SyntaxSet` grammars and `RegisteredSyntax` entries.
pub struct SyntaxRegistry {
    syntax_set: SyntaxSet,
    definitions: Vec<RegisteredSyntax>,
}

impl SyntaxRegistry {
    /// Loads the default `SyntaxRegistry` from `assets/syntaxes/*.sublime-syntax` on disk
    /// (falling back to embedded `.sublime-syntax` definitions if run from another working directory).
    pub fn load_default() -> Self {
        let candidates = [
            PathBuf::from("assets/syntaxes"),
            PathBuf::from(r"D:\github\rust\notepad\assets\syntaxes"),
        ];
        for dir in &candidates {
            if dir.is_dir() {
                if let Ok(reg) = Self::load_from_dir(dir) {
                    return reg;
                }
            }
        }
        Self::load_embedded()
    }

    /// Loads all `.sublime-syntax` files from `dir` into a compiled `SyntaxRegistry`.
    pub fn load_from_dir(dir: &Path) -> Result<Self, String> {
        let mut builder = SyntaxSetBuilder::new();
        builder.add_plain_text_syntax();

        let mut definitions = vec![RegisteredSyntax {
            name: String::from("Plain Text"),
            file_extensions: vec![String::from("txt"), String::from("log")],
            scope: String::from("text.plain"),
            first_line_match: None,
            loader: String::from("builtin:plain_text"),
        }];

        let mut loaded_names = std::collections::HashSet::new();
        loaded_names.insert(String::from("Plain Text"));

        let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("sublime-syntax"))
                    .unwrap_or(false)
            })
            .collect();
        paths.sort();

        for path in paths {
            let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let loader_id = path.to_string_lossy().to_string();
            Self::add_syntax_str(&mut builder, &mut definitions, &mut loaded_names, &raw, &loader_id);
        }

        // Ensure the 3 core languages are always present even if a directory was partial
        for (raw, loader_id) in [
            (EMBEDDED_RUST_SYNTAX, "embedded:Rust.sublime-syntax"),
            (EMBEDDED_JS_SYNTAX, "embedded:JavaScript.sublime-syntax"),
            (EMBEDDED_PYTHON_SYNTAX, "embedded:Python.sublime-syntax"),
        ] {
            Self::add_syntax_str(&mut builder, &mut definitions, &mut loaded_names, raw, loader_id);
        }

        let syntax_set = builder.build();
        Self::warm_up_syntax_set(&syntax_set);
        Ok(Self {
            syntax_set,
            definitions,
        })
    }

    /// Builds a `SyntaxRegistry` from the embedded `.sublime-syntax` definitions.
    pub fn load_embedded() -> Self {
        let mut builder = SyntaxSetBuilder::new();
        builder.add_plain_text_syntax();

        let mut definitions = vec![RegisteredSyntax {
            name: String::from("Plain Text"),
            file_extensions: vec![String::from("txt"), String::from("log")],
            scope: String::from("text.plain"),
            first_line_match: None,
            loader: String::from("builtin:plain_text"),
        }];
        let mut loaded_names = std::collections::HashSet::new();
        loaded_names.insert(String::from("Plain Text"));

        for (raw, loader_id) in [
            (EMBEDDED_RUST_SYNTAX, "assets/syntaxes/Rust.sublime-syntax"),
            (EMBEDDED_JS_SYNTAX, "assets/syntaxes/JavaScript.sublime-syntax"),
            (EMBEDDED_PYTHON_SYNTAX, "assets/syntaxes/Python.sublime-syntax"),
        ] {
            Self::add_syntax_str(&mut builder, &mut definitions, &mut loaded_names, raw, loader_id);
        }

        let syntax_set = builder.build();
        Self::warm_up_syntax_set(&syntax_set);
        Self {
            syntax_set,
            definitions,
        }
    }

    /// Pre-compiles lazy `syntect` context regular expressions during registry load so
    /// tokenization on the editor viewport never pays one-time regex compilation latency.
    fn warm_up_syntax_set(syntax_set: &SyntaxSet) {
        for syn in syntax_set.syntaxes() {
            let mut state = ParseState::new(syn);
            let _ = state.parse_line(
                "pub fn warm_up(x: usize) -> usize { return x + 42; } // comment\n",
                syntax_set,
            );
            let _ = state.parse_line(
                "function warmUp(x) { const a = \"str\"; return a; }\n",
                syntax_set,
            );
            let _ = state.parse_line(
                "@dec\ndef warm_up(x):\n    '''doc'''\n    return 0x10\n",
                syntax_set,
            );
            let _ = state.parse_line("/* block */ \"str\" 'c' \"\"\"triple\"\"\"\n", syntax_set);
        }
    }

    fn add_syntax_str(
        builder: &mut SyntaxSetBuilder,
        definitions: &mut Vec<RegisteredSyntax>,
        loaded_names: &mut std::collections::HashSet<String>,
        raw_yaml: &str,
        loader_id: &str,
    ) {
        let clean_yaml = raw_yaml
            .trim_start_matches('\u{feff}')
            .lines()
            .filter(|line| !line.trim_start().starts_with("%YAML"))
            .collect::<Vec<_>>()
            .join("\n");

        let header: SublimeSyntaxYamlHeader =
            serde_yaml::from_str(&clean_yaml).unwrap_or(SublimeSyntaxYamlHeader {
                name: None,
                file_extensions: Vec::new(),
                first_line_match: None,
                scope: None,
            });

        if let Ok(syn_def) = SyntectSyntaxDef::load_from_str(raw_yaml, true, None) {
            let name = header.name.unwrap_or_else(|| syn_def.name.clone());
            if loaded_names.contains(&name) {
                return;
            }
            let exts = if !header.file_extensions.is_empty() {
                header.file_extensions
            } else {
                syn_def.file_extensions.clone()
            };
            let scope = header
                .scope
                .unwrap_or_else(|| syn_def.scope.build_string());
            let first_line = header
                .first_line_match
                .or_else(|| syn_def.first_line_match.clone());

            builder.add(syn_def);
            loaded_names.insert(name.clone());
            definitions.push(RegisteredSyntax {
                name,
                file_extensions: exts,
                scope,
                first_line_match: first_line,
                loader: loader_id.to_string(),
            });
        }
    }

    /// Returns the underlying `syntect::parsing::SyntaxSet`.
    pub fn syntax_set(&self) -> &SyntaxSet {
        &self.syntax_set
    }

    /// Returns all registered syntax definitions.
    pub fn definitions(&self) -> &[RegisteredSyntax] {
        &self.definitions
    }

    /// Looks up a `RegisteredSyntax` by case-insensitive name (e.g., `"Rust"`, `"JavaScript"`, `"Python"`).
    pub fn find_by_name(&self, name: &str) -> Option<&RegisteredSyntax> {
        self.definitions
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(name))
    }

    /// Looks up a `RegisteredSyntax` by root scope (e.g., `"source.rust"`).
    pub fn find_by_scope(&self, scope: &str) -> Option<&RegisteredSyntax> {
        self.definitions
            .iter()
            .find(|d| d.scope.eq_ignore_ascii_case(scope))
    }

    /// Resolves a `syntect::parsing::SyntaxReference` for the given syntax `name`,
    /// falling back to `Plain Text`.
    pub fn syntax_ref_by_name(&self, name: &str) -> &SyntaxReference {
        self.syntax_set
            .find_syntax_by_name(name)
            .or_else(|| {
                self.find_by_name(name)
                    .and_then(|d| d.file_extensions.first())
                    .and_then(|ext| self.syntax_set.find_syntax_by_extension(ext))
            })
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text())
    }

    /// Detects the appropriate `RegisteredSyntax` for a file based on:
    /// 1. File extension in `path`
    /// 2. First-line shebang / header regex match against `first_line`
    /// 3. Fallback to `"Plain Text"`
    pub fn detect_syntax(&self, path: Option<&Path>, first_line: &str) -> &RegisteredSyntax {
        // 1. Check file extension
        if let Some(ext) = path.and_then(|p| p.extension()).and_then(|e| e.to_str()) {
            if let Some(def) = self.definitions.iter().find(|d| {
                d.file_extensions
                    .iter()
                    .any(|fe| fe.eq_ignore_ascii_case(ext))
            }) {
                return def;
            }
        }

        // 2. Check first-line shebang / header match
        let trimmed_first = first_line.trim();
        if !trimmed_first.is_empty() {
            if let Some(syn_ref) = self.syntax_set.find_syntax_by_first_line(trimmed_first) {
                if let Some(def) = self.find_by_name(&syn_ref.name) {
                    return def;
                }
            }
            // Also check common shebang keywords directly for fast deterministic matching
            if trimmed_first.starts_with("#!") {
                let lower = trimmed_first.to_lowercase();
                if lower.contains("python") {
                    if let Some(def) = self.find_by_name("Python") {
                        return def;
                    }
                }
                if lower.contains("node") || lower.contains("deno") || lower.contains("bun") {
                    if let Some(def) = self.find_by_name("JavaScript") {
                        return def;
                    }
                }
                if lower.contains("rust") {
                    if let Some(def) = self.find_by_name("Rust") {
                        return def;
                    }
                }
            }
        }

        // 3. Fallback to Plain Text
        &self.definitions[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syntax_registry_loads_and_detects_by_extension_and_shebang() {
        let reg = SyntaxRegistry::load_default();
        assert!(reg.find_by_name("Rust").is_some());
        assert!(reg.find_by_name("JavaScript").is_some());
        assert!(reg.find_by_name("Python").is_some());

        // Extension detection
        let rust_def = reg.detect_syntax(Some(Path::new("src/main.rs")), "");
        assert_eq!(rust_def.name(), "Rust");
        assert_eq!(rust_def.scope(), "source.rust");

        let js_def = reg.detect_syntax(Some(Path::new("app.mjs")), "");
        assert_eq!(js_def.name(), "JavaScript");
        assert_eq!(js_def.scope(), "source.js");

        let py_def = reg.detect_syntax(Some(Path::new("script.py")), "");
        assert_eq!(py_def.name(), "Python");
        assert_eq!(py_def.scope(), "source.python");

        // First-line shebang detection on extensionless files
        let shebang_py = reg.detect_syntax(None, "#!/usr/bin/env python3");
        assert_eq!(shebang_py.name(), "Python");

        let shebang_js = reg.detect_syntax(None, "#!/usr/bin/env node");
        assert_eq!(shebang_js.name(), "JavaScript");
    }
}
