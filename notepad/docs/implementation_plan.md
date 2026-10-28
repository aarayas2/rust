# Implementation Plan: Incremental Syntax Highlighting, Syntax Registry & Color Themes

## 1. Proposed Crate Stack (`Cargo.toml`)

| Crate | Version | Purpose |
| :--- | :--- | :--- |
| `syntect` | `"5"` | Sublime Text `.sublime-syntax` YAML compilation, `ParseState` + `ScopeStack` incremental lexing |
| `tree-sitter` | `"0.24"` | Incremental AST parser runtime (fallback / structural validation engine) |
| `tree-sitter-highlight` | `"0.24"` | Tree-sitter highlight query runner fallback |
| `ropey` | `"1"` | `Rope` text buffer for `O(log N)` line indexing and slicing on 10,000+ line documents |
| `serde` | `{ version = "1", features = ["derive"] }` | Structured deserialization for `.sublime-color-scheme`, `.tmTheme`, and `.sublime-syntax` metadata |
| `serde_json` | `"1"` | `.sublime-color-scheme` JSON parser with `variables` (`var(...)`) resolution |
| `serde_yaml` | `"0.9"` | `.sublime-syntax` YAML header & metadata inspection |
| `plist` | `"1"` | `.tmTheme` XML Property List parser |
| `once_cell` | `"1"` | Lazy static initialization of global default `SyntaxRegistry` and `ThemeRegistry` |

---

## 2. Module Architecture

```
D:\github\rust\notepad\
├── assets\
│   ├── syntaxes\
│   │   ├── Rust.sublime-syntax
│   │   ├── JavaScript.sublime-syntax
│   │   └── Python.sublime-syntax
│   └── themes\
│       ├── Monokai.tmTheme
│       └── Mariana.sublime-color-scheme
├── test_files\
│   ├── sample.rs
│   ├── sample.js
│   └── sample.py
└── src\
    ├── highlight\
    │   ├── mod.rs          # Highlighter trait, IncrementalHighlightCache, BracketMatcher, Rope buffer sync
    │   ├── tokenizer.rs    # SyntectTokenizer (wraps syntect ParseState/ScopeStack) + TreeSitterFallback
    │   └── registry.rs     # SyntaxDefinition trait, CustomSyntaxEntry, SyntaxRegistry (assets/syntaxes/ + detection)
    └── theme\
        ├── mod.rs          # Theme, Style, ColorScheme, ScopeRule, RgbColor, ThemePreset, GutterLayout
        ├── loader.rs       # .tmTheme (plist) & .sublime-color-scheme (serde_json) parsers + var() resolver
        └── manager.rs      # ThemeRegistry (loads assets/themes/ + built-in themes + live switching)
```

---

## 3. Detailed Component Design

### 3.1 `src/highlight/registry.rs` — Syntax Definitions & File Detection

```rust
use std::path::Path;
use syntect::parsing::SyntaxReference;

/// Trait representing a language syntax definition in the registry.
pub trait SyntaxDefinition: Send + Sync {
    /// Human-readable syntax name (e.g., `"Rust"`, `"JavaScript"`, `"Python"`).
    fn name(&self) -> &str;
    /// File extensions associated with this syntax (e.g., `["rs"]`, `["js", "mjs"]`).
    fn file_extensions(&self) -> &[String];
    /// Root TextMate/Sublime scope (e.g., `"source.rust"`, `"source.js"`, `"source.python"`).
    fn scope(&self) -> &str;
    /// Optional first-line regex pattern for shebang / header detection.
    fn first_line_match(&self) -> Option<&str>;
    /// Loader/origin identifier (`"assets/syntaxes/..."` or `"builtin"`).
    fn loader(&self) -> &str;
}
```

- **`SyntaxRegistry`:**
  - Scans `D:\github\rust\notepad\assets\syntaxes\*.sublime-syntax` (and relative `assets/syntaxes`), compiling them into a `syntect::parsing::SyntaxSetBuilder` alongside built-in defaults.
  - Implements:
    - `detect_syntax(path: Option<&Path>, first_line: &str) -> & dyn SyntaxDefinition`:
      1. Matches `path.extension()` case-insensitively against `file_extensions()`.
      2. Evaluates `first_line` against `first_line_match` patterns (e.g. `#!/usr/bin/env python`, `#!/usr/bin/env node`, `#!/usr/bin/env rust-script`).
      3. Falls back to `"Plain Text"` (`text.plain`).
    - `find_by_name(name: &str)` and `find_by_scope(scope: &str)`.

### 3.2 `src/highlight/tokenizer.rs` & `src/highlight/mod.rs` — Incremental Highlighting & Rope Cache

```rust
use ropey::Rope;
use std::ops::Range;

/// A single styled token span within a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightToken {
    /// Byte range relative to the start of the line.
    pub range: Range<usize>,
    /// Resolved dot-separated scope string (e.g., `"keyword.control.rust"`, `"comment.line.double-slash.rust"`).
    pub scope: String,
}

/// Trait for line-by-line incremental syntax highlighters.
pub trait Highlighter {
    /// Ensures lines `0..=end_line` have up-to-date token spans for `rope`, using cached line checkpoints.
    fn highlight_range(&mut self, rope: &Rope, start_line: usize, end_line: usize) -> &[Vec<HighlightToken>];
    /// Invalidates cached parser states starting from `from_line`.
    fn invalidate_from_line(&mut self, from_line: usize);
}
```

- **Incremental Convergence Algorithm (`IncrementalHighlightCache`):**
  - Stores a `ropey::Rope` mirror of the document plus `line_checkpoints: Vec<(ParseState, ScopeStack)>` and `line_tokens: Vec<Vec<HighlightToken>>`.
  - When an edit occurs (single-cursor or multi-cursor), computes the first modified line `first_diff_line` by comparing lines in the old `Rope` vs new `Rope` and calls `invalidate_from_line(first_diff_line)`.
  - During rendering or query of visible lines `first_visible..=last_visible`:
    - Re-tokenizes from `valid_up_to` using `SyntectTokenizer`.
    - If re-tokenizing an edited line produces the exact same `(ParseState, ScopeStack)` exit state as was previously cached for `line + 1`, the remainder of the cache is immediately valid (`O(1)` lines re-lexed on typical typing).
    - Guarantees `< 16ms` tokenization for 10,000+ lines and `< 1ms` per keystroke on the UI thread.
- **Bracket Matching (`find_matching_bracket`):**
  - Given the primary caret byte offset in `Rope`, inspects the character at `caret` or `caret - 1` for `(`, `)`, `{`, `}`, `[`, `]` and scans forward/backward (ignoring brackets inside `comment.*` or `string.*` scopes) to return `Some((open_byte, close_byte))`.

### 3.3 `src/theme/mod.rs`, `src/theme/loader.rs`, `src/theme/manager.rs` — Color Themes

- **`Style` & `ColorScheme` (`src/theme/mod.rs`):**
  - `Style { pub foreground: RgbColor, pub background: Option<RgbColor>, pub bold: bool, pub italic: bool, pub underline: bool }`.
  - `ScopeRule { pub name: String, pub selectors: Vec<String>, pub style: Style }`.
  - `Theme` retains all existing fields (`name`, `is_dark`, `background`, `foreground`, `caret`, `secondary_caret`, `selection`, `selection_border`, `line_highlight`, `gutter`, `gutter_foreground`, `gutter_foreground_highlight`, `status_background`, `status_foreground`) **plus** `rules: Vec<ScopeRule>` and `brackets_foreground: RgbColor`.
  - `Theme::style_for_scope(&self, scope: &str) -> Style`: scores each `ScopeRule` selector by hierarchical prefix match length against the token's scope stack (`comment.line.double-slash.rust` matches `comment.line` with score 2 and `comment` with score 1) and returns the highest-specificity `Style`.
- **Theme Loaders (`src/theme/loader.rs`):**
  - `load_sublime_color_scheme_str(json_str: &str) -> Result<Theme, String>`: parses `.sublime-color-scheme` JSON via `serde_json`, resolves `variables` (`var(name)`), parses `#RRGGBB` / `#RRGGBBAA` / `rgb(...)` / `hsl(...)` colors, extracts `globals` and `rules`.
  - `load_tm_theme_str(xml_str: &str) -> Result<Theme, String>`: parses `.tmTheme` XML Plist via `plist::Value`, extracts global settings dict and per-scope rule dicts.
- **`ThemeRegistry` (`src/theme/manager.rs`):**
  - Loads all `.sublime-color-scheme` and `.tmTheme` files from `D:\github\rust\notepad\assets\themes\` and registers them alongside built-in presets for instant live switching via `Preferences > Select Theme` (`Format > Theme`).

---

## 4. UI & Command Integration (`src/window.rs`, `src/menu.rs`, `src/tabs.rs`)

1. **Menu Commands (`src/menu.rs`):**
   - `View > Select Syntax`:
     - `ID_VIEW_SYNTAX_RUST` (`Rust`)
     - `ID_VIEW_SYNTAX_JS` (`JavaScript`)
     - `ID_VIEW_SYNTAX_PYTHON` (`Python`)
     - `ID_VIEW_SYNTAX_PLAIN` (`Plain Text`)
   - `Preferences > Select Theme` (and `Format > Theme`):
     - `ID_FORMAT_THEME_MARIANA` (`Mariana (assets/themes/Mariana.sublime-color-scheme)`)
     - `ID_FORMAT_THEME_MONOKAI` (`Monokai (assets/themes/Monokai.tmTheme)`)
     - `ID_FORMAT_THEME_BREAKERS` (`Breakers Light`)
     - `ID_FORMAT_THEME_CLASSIC` (`Classic Notepad Light`)
2. **Editor Token & Bracket Rendering (`src/window.rs`):**
   - Each `Tab` (`src/tabs.rs`) stores `syntax_name: String` (auto-detected on file open or first line edit).
   - `NotepadAppState` holds `syntax_registry: Arc<SyntaxRegistry>`, `theme_registry: ThemeRegistry`, and `highlight_cache: IncrementalHighlightCache`.
   - In `edit_subclass_proc` (`WM_PAINT`), after the base control paints:
     - `render_syntax_highlighted_viewport()` queries the visible line range (`first_visible..=last_visible`), fetches cached token spans from `highlight_cache`, and draws each colored token (with bold/italic font variants for keywords/comments) + matching bracket underlines directly onto `hwnd_edit`, followed by `render_multi_cursor_and_block_overlays()`.
   - Status bar displays the active syntax name (`| Rust |`, `| JavaScript |`, `| Python |`, `| Plain Text |`) and active theme name.
