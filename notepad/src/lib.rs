//! # Notepad Core Win32 Reverse-Engineered Application & Library
//!
//! This crate contains a complete, clean-room, safe, and idiomatic Rust reimplementation
//! of classic Windows Notepad (`C:\Windows\System32\notepad.exe`) upgraded with
//! Sublime Text-inspired multi-tab, folder sidebar, multi-cursor, incremental syntax
//! highlighting, extensible syntax definitions, and `.sublime-color-scheme` / `.tmTheme`
//! color themes:
//! - **Window Class & `WndProc` (`window.rs`):** Recreates `NPRegister`, `NPInit`,
//!   `NPWndProc`, `NPSize`, and `NPCommand`.
//! - **Incremental Syntax Highlighting (`highlight/`):** Ropey + Syntect `.sublime-syntax`
//!   tokenizer + Tree-sitter fallback + `O(1)` line-state convergence cache.
//! - **Color Theme Engine (`theme/`):** Parses `.sublime-color-scheme` (JSON) and `.tmTheme`
//!   (XML Plist) files from `assets/themes/` with live theme switching.
//! - **File I/O & Encoding Engine (`file_io.rs`):** BOM detection, embedded-NUL sanitization,
//!   `.LOG` timestamp injection, and `EM_SETMODIFY` state.
//! - **Menu Bar & State Synchronization (`menu.rs`):** 6-submenu menu bar and `InitMenu`.
//! - **Word Wrap & Edit Recreation (`word_wrap.rs`):** Toggles `WS_HSCROLL` (`0x00100000`).
//! - **Common File Dialogs (`save_dialog.rs`):** Safe builders around `GetOpenFileNameW`
//!   and `GetSaveFileNameW`.

pub mod cursor;
pub mod file_explorer;
pub mod file_io;
pub mod font;
pub mod highlight;
pub mod menu;
pub mod save_dialog;
pub mod tabs;
pub mod theme;
pub mod window;
pub mod word_wrap;

pub use cursor::{
    byte_to_utf16_offset, utf16_to_byte_offset, BlockDirection, Cursor, CursorId, Cursors,
    MoveDirection, MultiCursorEditor, Position2D, SelectionActiveSet, SelectionContext,
    SelectionMode,
};
pub use file_explorer::{
    FileExplorer, FileNode, FlatTreeRow, SidebarHitTarget, DEFAULT_SIDEBAR_WIDTH,
};
pub use file_io::{
    decode_notepad_bytes, encode_notepad_bytes, get_edit_text, is_edit_modified, load_file_win32,
    save_file_win32, set_edit_modified, set_edit_text, LoadedDocument, TextEncoding,
};
pub use font::{
    format_sublime_window_title, FontState, DEFAULT_WINDOWS_FONT_FACE, DEFAULT_WINDOWS_FONT_SIZE,
    MAX_FONT_SIZE, MIN_MENU_FONT_SIZE, MIN_VIEW_FONT_SIZE,
};
pub use highlight::{
    find_matching_bracket, HighlightToken, Highlighter, IncrementalHighlightCache, LineLexerState,
    RegisteredSyntax, SyntaxDefinition, SyntaxRegistry, SyntectTokenizer, TreeSitterFallback,
    GLOBAL_SYNTAX_REGISTRY,
};
pub use menu::{create_notepad_menu, init_menu, EditorState};
pub use save_dialog::{OpenFileDialog, SaveDialogError, SaveFileDialog};
pub use tabs::{Tab, TabHitTarget, TabManager, TabRect, TAB_BAR_HEIGHT};
pub use theme::{
    ColorScheme, GutterLayout, RgbColor, ScopeRule, Style, Theme, ThemePreset, ThemeRegistry,
    GLOBAL_THEME_REGISTRY,
};
pub use window::{create_notepad_window, notepad_wnd_proc, register_notepad_class, NotepadAppState};
pub use word_wrap::{WordWrap, WordWrapManager};
