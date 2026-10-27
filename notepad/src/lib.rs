//! # Notepad Core Win32 Reverse-Engineered Application & Library
//!
//! This crate contains a complete, clean-room, safe, and idiomatic Rust reimplementation
//! of classic Windows Notepad (`C:\Windows\System32\notepad.exe`):
//! - **Window Class & `WndProc` (`window.rs`):** Recreates `NPRegister` (`0x14001E28F`),
//!   `NPInit` (`0x1400134DC` & `0x1400125E8`), `NPWndProc` (`0x14000E950`), `NPSize`
//!   (`0x14000E848`), and `NPCommand` (`0x14000DA7C`).
//! - **File I/O & Encoding Engine (`file_io.rs`):** Recreates `LoadFile` (`0x140010F48`)
//!   and `SaveFile` (`0x140011AF8`), including BOM detection, embedded-NUL sanitization
//!   (`0x1400115AE`), `.LOG` timestamp injection (`0x1400115C2`), and `EM_SETMODIFY` state.
//! - **Menu Bar & State Synchronization (`menu.rs`):** Recreates the 5-submenu Notepad menu
//!   bar and the 782-byte `InitMenu` subroutine (`0x14000F2A0`) called during `WM_INITMENU`.
//! - **Word Wrap & Edit Recreation (`word_wrap.rs`):** Recreates `CheckMenuItem` (`0x14000F55A`)
//!   and `NPReCreate` (`0x14001FA28`) to toggle `WS_HSCROLL` (`0x00100000`).
//! - **Common File Dialogs (`save_dialog.rs`):** Safe builders around `GetOpenFileNameW`
//!   (`0x14000D078`) and `GetSaveFileNameW` (`0x14000D2E8`).

pub mod cursor;
pub mod file_io;
pub mod menu;
pub mod save_dialog;
pub mod theme;
pub mod window;
pub mod word_wrap;

pub use cursor::{
    byte_to_utf16_offset, utf16_to_byte_offset, BlockDirection, Cursor, CursorId, Cursors,
    MoveDirection, MultiCursorEditor, Position2D, SelectionActiveSet, SelectionContext,
    SelectionMode,
};
pub use file_io::{
    decode_notepad_bytes, encode_notepad_bytes, get_edit_text, is_edit_modified, load_file_win32,
    save_file_win32, set_edit_modified, set_edit_text, LoadedDocument, TextEncoding,
};
pub use menu::{create_notepad_menu, init_menu, EditorState};
pub use save_dialog::{OpenFileDialog, SaveDialogError, SaveFileDialog};
pub use theme::{GutterLayout, RgbColor, Theme, ThemePreset};
pub use window::{create_notepad_window, notepad_wnd_proc, register_notepad_class, NotepadAppState};
pub use word_wrap::{WordWrap, WordWrapManager};
