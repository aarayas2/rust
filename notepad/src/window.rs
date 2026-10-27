//! Reverse-engineered Notepad Window Procedure (`NPWndProc`), Class Registration (`NPRegister`),
//! Child Control Lifecycle (`NPInit` / `NPSize`), Command Dispatch (`NPCommand`),
//! and Multi-Cursor / Block Selection Subclass & GDI Overlay Renderer.
//!
//! # Reverse Engineering Notes
//!
//! - **`NPRegister` (`0x14001E200` - `0x14001E2A0`):**
//!   - Registers class `"Notepad"` (`0x14002B828`) via `RegisterClassExW` (`0x14001E28F`).
//!   - Sets `lpfnWndProc = 0x14000E950`, `hCursor = LoadCursorW(NULL, IDC_IBEAM = 0x7F01)`,
//!     `hbrBackground = (HBRUSH)(COLOR_WINDOW + 1)` (`6`).
//! - **`NPInit` (`0x140013488` - `0x140013780`):**
//!   - Calls `CreateWindowExW` (`0x1400134DC`) with `WS_OVERLAPPEDWINDOW` (`0x00CF0000`) for main window.
//!   - Calls `CreateWindowExW` (`0x1400125E8`) for child `"Edit"` control (ID `0x0F`).
//!   - Creates status bar window (ID `0x401` at `0x140013701`).
//! - **`NPWndProc` (`0x14000E950` - `0x14000F140`):**
//!   - `WM_DESTROY` (`0x0002`) -> `0x14000EAAD`: calls `PostQuitMessage(0)` (`0x14000EAAF`).
//!   - `WM_SIZE` (`0x0005`) -> `0x14000EA6E`: calls `NPSize` (`0x14000E848`), which calls
//!     `MoveWindow(g_hwndEdit, 0, 0, cx, cy - statusHeight, TRUE)` (`0x14000E8ED`).
//!   - `WM_SETFOCUS` (`0x0007`) -> `0x14000E9E5`: calls `!IsIconic` and `SetFocus(g_hwndEdit)` (`0x14000EA07`).
//!   - `WM_CLOSE` (`0x0010`) -> `0x14000EAC0`: calls `CheckSave` (`0x14000C520`) and `DestroyWindow` (`0x14000EB0B`).
//!   - `WM_COMMAND` (`0x0111`) -> `0x14000EF8F`: checks `EN_CHANGE` (`0x0300`) from `g_hwndEdit`,
//!     then invokes `NPCommand` (`0x14000DA7C`).
//!   - `WM_INITMENU` (`0x0116`) -> `0x14000ED6D`: invokes `InitMenu` (`0x14000F2A0`).

use crate::cursor::{
    byte_to_utf16_offset, utf16_to_byte_offset, BlockDirection, MoveDirection, MultiCursorEditor,
};
use crate::file_io::{
    get_edit_text, is_edit_modified, load_file_win32, save_file_win32, set_edit_modified,
    set_edit_text, TextEncoding,
};
use crate::menu::{
    create_notepad_menu, init_menu, EditorState, EM_GETSEL, EM_REPLACESEL, EM_SCROLLCARET,
    EM_SETSEL, EM_UNDO, IDM_FILEEXIT, IDM_FILENEW, IDM_FILEOPEN, IDM_FILESAVE, IDM_FILESAVEAS,
    IDM_HELP_ABOUT, ID_EDIT_ADD_CURSORS_LINE_ENDS, ID_EDIT_ADD_CURSOR_ABOVE,
    ID_EDIT_ADD_CURSOR_BELOW, ID_EDIT_ADD_CURSOR_NEXT_MATCH, ID_EDIT_CLEAR, ID_EDIT_COPY,
    ID_EDIT_CUT, ID_EDIT_PASTE, ID_EDIT_SELECTALL, ID_EDIT_SINGLE_CURSOR, ID_EDIT_TIMEDATE,
    ID_EDIT_UNDO, ID_FORMAT_THEME_BREAKERS, ID_FORMAT_THEME_CLASSIC, ID_FORMAT_THEME_MARIANA,
    ID_FORMAT_THEME_MONOKAI, ID_VIEW_LINE_NUMBERS, ID_VIEW_STATUS,
};
use crate::save_dialog::{OpenFileDialog, SaveFileDialog};
use crate::theme::{GutterLayout, Theme, ThemePreset};
use crate::word_wrap::{create_edit_control, WordWrap, WordWrapManager, ID_FORMAT_WORDWRAP};
use std::collections::HashSet;
use std::path::PathBuf;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, GetDC, GetStockObject,
    GetTextExtentPoint32W, GetTextMetricsW, InvalidateRect, InvertRect, ReleaseDC, SelectObject,
    SetBkColor, SetBkMode, SetTextColor, TextOutW, COLOR_WINDOW, DEFAULT_GUI_FONT, HBRUSH, HDC,
    HGDIOBJ, PAINTSTRUCT, TEXTMETRICW, TRANSPARENT,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME,
    VK_LEFT, VK_MENU, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_SHIFT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect,
    GetWindowLongPtrW, IsIconic, LoadCursorW, LoadIconW, MessageBoxW, MoveWindow, PostMessageW,
    PostQuitMessage, RegisterClassExW, SendMessageW, SetWindowLongPtrW, SetWindowTextW, ShowWindow,
    CW_USEDEFAULT, GWLP_USERDATA, GWLP_WNDPROC, HMENU, IDC_IBEAM, IDI_APPLICATION, IDNO, IDYES,
    MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MB_YESNOCANCEL, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CHAR, WM_CLEAR, WM_CLOSE, WM_COMMAND, WM_COPY, WM_CTLCOLOREDIT,
    WM_CTLCOLORSTATIC, WM_CUT, WM_DESTROY, WM_ERASEBKGND, WM_GETFONT, WM_INITMENU, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_PASTE, WM_SETFOCUS,
    WM_SETFONT, WM_SIZE, WM_SYSCHAR, WM_SYSKEYDOWN, WM_VSCROLL, WNDCLASSEXW, WNDPROC, WS_CHILD,
    WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

/// Window Class Name (`0x14002B828`: `"Notepad"`).
pub const NOTEPAD_CLASS_NAME: &str = "Notepad\0";

/// Child control ID for the Status Bar (`0x401` at `0x140013701`).
pub const ID_STATUS_BAR: usize = 0x401;

/// Default height of the status bar in pixels.
pub const STATUS_BAR_HEIGHT: i32 = 24;

/// Edit control notification code for text modification (`EN_CHANGE = 0x0300` at `0x14000EF9B`).
pub const EN_CHANGE: u16 = 0x0300;

/// Edit control message to set maximum text length (`EM_LIMITTEXT = 0x00C5`).
pub const EM_LIMITTEXT: u32 = 0x00C5;

/// Edit control message to query the zero-based index of the uppermost visible line (`EM_GETFIRSTVISIBLELINE = 0x00CE`).
pub const EM_GETFIRSTVISIBLELINE: u32 = 0x00CE;

/// Edit control message to query client `(x, y)` coordinates of a character index (`EM_POSFROMCHAR = 0x00D6`).
pub const EM_POSFROMCHAR: u32 = 0x00D6;

/// Edit control message to query nearest character index from client `(x, y)` coordinates (`EM_CHARFROMPOS = 0x00D7`).
pub const EM_CHARFROMPOS: u32 = 0x00D7;

/// Mouse button state mask for Left Mouse Button (`MK_LBUTTON = 0x0001`).
const MK_LBUTTON_MASK: usize = 0x0001;

/// Complete runtime state for a Notepad window instance, stored safely in `GWLP_USERDATA`.
pub struct NotepadAppState {
    pub hinstance: HINSTANCE,
    pub hwnd_main: HWND,
    pub hwnd_edit: HWND,
    pub hwnd_status: HWND,
    pub word_wrap: WordWrapManager,
    pub status_bar_visible: bool,
    pub current_file: Option<PathBuf>,
    pub encoding: TextEncoding,
    /// Multi-cursor and rectangular block-selection state engine.
    pub multi_cursor: MultiCursorEditor,
    /// Original Win32 `WndProc` of the subclassed `"Edit"` control.
    pub edit_prev_wndproc: isize,
    /// Origin byte offset when performing an `Alt+Drag` rectangular block selection.
    pub alt_drag_origin: Option<usize>,
    /// Internal clipboard buffer for multi-cursor and rectangular block selections.
    pub internal_clipboard: Option<String>,
    /// Guard flag to avoid recursive `EN_CHANGE` resynchronization while applying multi-cursor edits.
    pub syncing_to_hwnd: bool,
    /// Active Sublime Text-inspired color scheme preset.
    pub theme_preset: ThemePreset,
    /// Active color palette (`background`, `foreground`, `caret`, `gutter`, `gutter_foreground`, etc.).
    pub theme: Theme,
    /// Cached GDI solid brush for `WM_CTLCOLOREDIT` (`theme.background`).
    pub edit_bg_brush: HBRUSH,
    /// Cached GDI solid brush for `WM_CTLCOLORSTATIC` (`theme.status_background`).
    pub status_bg_brush: HBRUSH,
    /// Whether the left line-number gutter is visible (`[rsi + 0x564]` / `[rsi + 0x566]` in `sublime_text.exe`).
    pub line_numbers_visible: bool,
    /// Computed left gutter layout (`digit_count`, `char_zero_width`, `total_width`).
    pub gutter_layout: GutterLayout,
}

impl NotepadAppState {
    /// Returns the current `EditorState` snapshot used by `init_menu` (`0x14000F2A0`).
    pub fn editor_state(&self) -> EditorState {
        EditorState {
            word_wrap: self.word_wrap.state(),
            status_bar_visible: self.status_bar_visible,
            line_numbers_visible: self.line_numbers_visible,
            theme_preset: self.theme_preset,
        }
    }

    /// Switches the active color scheme preset, recreates GDI background brushes for
    /// `WM_CTLCOLOREDIT` / `WM_CTLCOLORSTATIC`, and repaints the editor, gutter, and status bar.
    pub fn set_theme_preset(&mut self, preset: ThemePreset) {
        self.theme_preset = preset;
        self.theme = Theme::from_preset(preset);

        unsafe {
            if !self.edit_bg_brush.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(self.edit_bg_brush.0));
            }
            if !self.status_bg_brush.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(self.status_bg_brush.0));
            }
            self.edit_bg_brush = CreateSolidBrush(self.theme.background.to_colorref());
            self.status_bg_brush = CreateSolidBrush(self.theme.status_background.to_colorref());

            if !self.hwnd_main.is_invalid() {
                let _ = InvalidateRect(Some(self.hwnd_main), None, true);
            }
            if !self.hwnd_edit.is_invalid() {
                let _ = InvalidateRect(Some(self.hwnd_edit), None, true);
            }
            if !self.hwnd_status.is_invalid() {
                let _ = InvalidateRect(Some(self.hwnd_status), None, true);
            }
        }
        self.update_status_bar();
        unsafe {
            self.render_gutter_direct();
        }
    }

    /// Toggles the visibility of the left line-number gutter and relayouts the viewport.
    pub fn toggle_line_numbers(&mut self) {
        self.line_numbers_visible = !self.line_numbers_visible;
        self.relayout_viewport();
        self.update_status_bar();
    }

    /// Measures the pixel advance width of ASCII `'0'` (`0x30`, matching `0x14036B586` in `sublime_text.exe`)
    /// and the line height of `self.hwnd_edit`'s active font via GDI `GetTextExtentPoint32W` and `GetTextMetricsW`.
    pub fn measure_edit_font_metrics(&self) -> (i32, i32) {
        if self.hwnd_edit.is_invalid() {
            return (8, 16);
        }
        unsafe {
            let hdc = GetDC(Some(self.hwnd_edit));
            if hdc.is_invalid() {
                return (8, 16);
            }

            let hfont = SendMessageW(self.hwnd_edit, WM_GETFONT, None, None);
            let old_font = if hfont.0 != 0 {
                SelectObject(hdc, HGDIOBJ(hfont.0 as *mut core::ffi::c_void))
            } else {
                let stock = GetStockObject(DEFAULT_GUI_FONT);
                if !stock.is_invalid() {
                    SelectObject(hdc, stock)
                } else {
                    HGDIOBJ::default()
                }
            };

            let mut tm = TEXTMETRICW::default();
            let _ = GetTextMetricsW(hdc, &mut tm);
            let line_height = tm.tmHeight.max(14);

            // Measure ASCII '0' (0x30) advance width via GetTextExtentPoint32W (Sublime 0x14036B586)
            let zero_glyph: [u16; 1] = [0x0030];
            let mut size = SIZE::default();
            let ok = GetTextExtentPoint32W(hdc, &zero_glyph, &mut size).as_bool();
            let char_zero_w = if ok && size.cx > 0 {
                size.cx
            } else {
                tm.tmAveCharWidth.max(7)
            };

            if !old_font.is_invalid() {
                let _ = SelectObject(hdc, old_font);
            }
            let _ = ReleaseDC(Some(self.hwnd_edit), hdc);
            (char_zero_w, line_height)
        }
    }

    /// Recomputes `self.gutter_layout` based on the current line count in `self.multi_cursor`.
    /// Returns `true` if the total pixel width of the gutter changed (e.g., crossing 99 -> 100 lines).
    pub fn refresh_gutter_layout(&mut self) -> bool {
        let total_lines = self.multi_cursor.line_count();
        let (char_zero_w, _) = self.measure_edit_font_metrics();
        let new_layout = GutterLayout::compute(
            total_lines,
            char_zero_w,
            GutterLayout::DEFAULT_MARGIN_PX,
            self.line_numbers_visible,
        );
        let width_changed = new_layout.total_width != self.gutter_layout.total_width
            || new_layout.digit_count != self.gutter_layout.digit_count;
        self.gutter_layout = new_layout;
        width_changed
    }

    /// Queries the main window client area and repositions the left gutter, `"Edit"` control, and status bar.
    pub fn relayout_viewport(&mut self) {
        if self.hwnd_main.is_invalid() {
            return;
        }
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd_main, &mut rc);
        }
        self.resize_children(rc.right - rc.left, rc.bottom - rc.top);
    }

    /// Returns the active status bar height (`0` when hidden, `STATUS_BAR_HEIGHT` when visible).
    pub fn active_status_height(&self) -> i32 {
        if self.status_bar_visible {
            STATUS_BAR_HEIGHT
        } else {
            0
        }
    }

    /// Synchronizes `self.multi_cursor` from the native `hwnd_edit` control when in single-cursor mode.
    pub fn sync_from_hwnd_edit(&mut self) {
        if self.syncing_to_hwnd || self.hwnd_edit.is_invalid() {
            return;
        }
        let current_text = get_edit_text(self.hwnd_edit);
        let mut sel_start: u32 = 0;
        let mut sel_end: u32 = 0;
        unsafe {
            SendMessageW(
                self.hwnd_edit,
                EM_GETSEL,
                Some(WPARAM(&mut sel_start as *mut u32 as usize)),
                Some(LPARAM(&mut sel_end as *mut u32 as isize)),
            );
        }
        self.multi_cursor.sync_from_single_selection(
            current_text,
            sel_start as usize,
            sel_end as usize,
        );
        if self.refresh_gutter_layout() {
            self.relayout_viewport();
        } else {
            unsafe {
                self.render_gutter_direct();
            }
        }
    }

    /// Pushes the current `self.multi_cursor` text buffer and primary caret/selection to `hwnd_edit`.
    pub fn sync_to_hwnd_edit(&mut self, mark_modified: bool) {
        if self.hwnd_edit.is_invalid() {
            return;
        }
        self.syncing_to_hwnd = true;

        let current_hwnd_text = get_edit_text(self.hwnd_edit);
        let target_text = self.multi_cursor.text();
        if current_hwnd_text != target_text {
            set_edit_text(self.hwnd_edit, target_text);
            if mark_modified {
                set_edit_modified(self.hwnd_edit, true);
            }
        }

        let primary = self.multi_cursor.cursors.primary();
        let (sel_start_byte, sel_end_byte) = if self.multi_cursor.cursors.is_multi()
            || self.multi_cursor.cursors.has_block_selection()
        {
            // When multiple cursors or custom block selection are active, collapse the native
            // single-selection highlight so our custom GDI overlay renders all selections cleanly.
            (primary.position, primary.position)
        } else if let Some(anchor) = primary.anchor {
            (anchor, primary.position)
        } else {
            (primary.position, primary.position)
        };

        let start_u16 = byte_to_utf16_offset(self.multi_cursor.text(), sel_start_byte);
        let end_u16 = byte_to_utf16_offset(self.multi_cursor.text(), sel_end_byte);

        unsafe {
            SendMessageW(
                self.hwnd_edit,
                EM_SETSEL,
                Some(WPARAM(start_u16)),
                Some(LPARAM(end_u16 as isize)),
            );
            SendMessageW(self.hwnd_edit, EM_SCROLLCARET, None, None);
            let _ = InvalidateRect(Some(self.hwnd_edit), None, false);
        }

        self.syncing_to_hwnd = false;
        if self.refresh_gutter_layout() {
            self.relayout_viewport();
        } else {
            unsafe {
                self.render_gutter_direct();
            }
        }
        self.update_window_title();
        self.update_status_bar();
    }

    /// Updates the main window title bar (`UpdateTitle` at `0x14000FE3C`), prefixing `*` when dirty.
    pub fn update_window_title(&self) {
        let file_name = self
            .current_file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string());

        let dirty_prefix = if is_edit_modified(self.hwnd_edit) {
            "*"
        } else {
            ""
        };
        let title = format!("{}{} - Notepad\0", dirty_prefix, file_name);
        let wide: Vec<u16> = title.encode_utf16().collect();
        unsafe {
            let _ = SetWindowTextW(self.hwnd_main, PCWSTR(wide.as_ptr()));
        }
    }

    /// Updates the status bar text (`Ln X, Col Y | [Cursors/Block] | 100% | Windows (CRLF) | UTF-8`).
    pub fn update_status_bar(&self) {
        if self.hwnd_status.is_invalid() || !self.status_bar_visible {
            return;
        }

        let (line, col) = if self.multi_cursor.cursors.is_multi()
            || self.multi_cursor.cursors.has_block_selection()
        {
            let pos = self.multi_cursor.cursors.primary().position;
            let pos_2d = self.multi_cursor.byte_to_2d(pos);
            let col_display = self
                .multi_cursor
                .cursors
                .primary()
                .sticky_column
                .unwrap_or(pos_2d.column)
                + 1;
            (pos_2d.line + 1, col_display)
        } else {
            let mut sel_start: u32 = 0;
            let mut sel_end: u32 = 0;
            unsafe {
                SendMessageW(
                    self.hwnd_edit,
                    EM_GETSEL,
                    Some(WPARAM(&mut sel_start as *mut u32 as usize)),
                    Some(LPARAM(&mut sel_end as *mut u32 as isize)),
                );
            }

            let text = get_edit_text(self.hwnd_edit);
            let prefix: String = text.encode_utf16().take(sel_end as usize).fold(
                String::new(),
                |mut acc, ch| {
                    if let Some(c) = char::from_u32(ch as u32) {
                        acc.push(c);
                    }
                    acc
                },
            );

            let line = prefix.matches('\n').count() + 1;
            let col = prefix
                .rsplit('\n')
                .next()
                .map(|s| s.trim_end_matches('\r').chars().count() + 1)
                .unwrap_or(1);
            (line, col)
        };

        let mode_badge = if self.multi_cursor.cursors.has_block_selection() {
            let ctx = self.multi_cursor.selection_context();
            if let Some(&(sl, sc, el, ec)) = ctx.block_rects.first() {
                format!("   |   Block {}x{}", el - sl + 1, ec.saturating_sub(sc))
            } else {
                "   |   Block Selection".to_string()
            }
        } else if self.multi_cursor.cursors.is_multi() {
            format!("   |   {} Cursors", self.multi_cursor.cursors.count())
        } else {
            String::new()
        };

        let enc_label = match self.encoding {
            TextEncoding::Utf8 => "UTF-8",
            TextEncoding::Utf8Bom => "UTF-8-BOM",
            TextEncoding::Utf16Le => "UTF-16 LE",
            TextEncoding::Utf16Be => "UTF-16 BE",
        };

        let status_str = format!(
            "  Ln {}, Col {}{}   |   {}   |   Windows (CRLF)   |   {}\0",
            line, col, mode_badge, self.theme.name, enc_label
        );
        let wide: Vec<u16> = status_str.encode_utf16().collect();
        unsafe {
            let _ = SetWindowTextW(self.hwnd_status, PCWSTR(wide.as_ptr()));
        }
    }

    /// Resizes the left line-number gutter, child `"Edit"` control, and status bar to fit the
    /// main window client area (`NPSize` at `0x14000E848` combined with Sublime Text's
    /// viewport split at `0x1403A1AC6` and `0x140369FA9`).
    pub fn resize_children(&mut self, width: i32, height: i32) {
        let status_h = self.active_status_height();
        let edit_h = (height - status_h).max(0);

        let _ = self.refresh_gutter_layout();
        let (gutter_w, edit_x, edit_w) = self.gutter_layout.split_viewport(width);

        unsafe {
            if !self.hwnd_edit.is_invalid() {
                // Sublime 0x1403A1AC6 & 0x140369FA9: offset text viewport by gutter_w and subtract gutter_w from width
                let _ = MoveWindow(self.hwnd_edit, edit_x, 0, edit_w, edit_h, true);
            }
            if !self.hwnd_status.is_invalid() && self.status_bar_visible {
                // 0x14000E915: MoveWindow(g_hwndStatus, 0, cy - statusHeight, cx, statusHeight, TRUE)
                let _ = MoveWindow(self.hwnd_status, 0, edit_h, width, status_h, true);
            }
            if !self.hwnd_main.is_invalid() && gutter_w > 0 {
                let gutter_rc = RECT {
                    left: 0,
                    top: 0,
                    right: gutter_w,
                    bottom: edit_h,
                };
                let _ = InvalidateRect(Some(self.hwnd_main), Some(&gutter_rc), false);
            }
            self.render_gutter_direct();
        }
    }

    /// Prompts the user to save changes if the document is modified (`CheckSave` at `0x14000C520`).
    /// Returns `true` if the caller may proceed, or `false` if the user cancelled.
    pub fn check_save(&mut self) -> bool {
        if !is_edit_modified(self.hwnd_edit) {
            return true;
        }

        let name = self
            .current_file
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "Untitled".to_string());

        let prompt = format!("Do you want to save changes to {}?\0", name);
        let caption: Vec<u16> = "Notepad\0".encode_utf16().collect();
        let prompt_wide: Vec<u16> = prompt.encode_utf16().collect();

        let response = unsafe {
            MessageBoxW(
                Some(self.hwnd_main),
                PCWSTR(prompt_wide.as_ptr()),
                PCWSTR(caption.as_ptr()),
                MB_YESNOCANCEL | MB_ICONWARNING,
            )
        };

        if response == IDYES {
            self.cmd_file_save()
        } else {
            response == IDNO
        }
    }

    /// Executes `IDM_FILENEW` (`0x01` -> `0x14000DF37` -> `New` at `0x1400118A0`).
    pub fn cmd_file_new(&mut self) {
        if !self.check_save() {
            return;
        }
        set_edit_text(self.hwnd_edit, "");
        self.multi_cursor.set_text("");
        self.current_file = None;
        self.encoding = TextEncoding::Utf8;
        if self.refresh_gutter_layout() {
            self.relayout_viewport();
        } else {
            unsafe {
                self.render_gutter_direct();
            }
        }
        self.update_window_title();
        self.update_status_bar();
    }

    /// Executes `IDM_FILEOPEN` (`0x02` -> `0x14000DE00` -> `LoadFile` at `0x140010F48`).
    pub fn cmd_file_open(&mut self) {
        if !self.check_save() {
            return;
        }

        match OpenFileDialog::new(self.hwnd_main).show() {
            Ok(Some(path)) => {
                let _ = self.open_path(path);
            }
            Ok(None) => {}
            Err(err) => {
                self.show_error(&format!("Failed to open file dialog: {}", err));
            }
        }
    }

    /// Loads a specific path into the editor (`LoadFile` at `0x140010F48`).
    pub fn open_path(&mut self, path: PathBuf) -> Result<(), String> {
        match load_file_win32(&path) {
            Ok(doc) => {
                set_edit_text(self.hwnd_edit, &doc.text);
                self.current_file = Some(path);
                self.encoding = doc.encoding;

                // 0x1400115C2: If document starts with ".LOG", append current timestamp at end
                if doc.is_log_file {
                    let end_len = doc.text.encode_utf16().count();
                    unsafe {
                        SendMessageW(
                            self.hwnd_edit,
                            EM_SETSEL,
                            Some(WPARAM(end_len)),
                            Some(LPARAM(end_len as isize)),
                        );
                    }
                    self.insert_date_time(true);
                }

                self.multi_cursor.set_text(get_edit_text(self.hwnd_edit));
                if self.refresh_gutter_layout() {
                    self.relayout_viewport();
                } else {
                    unsafe {
                        self.render_gutter_direct();
                    }
                }
                self.update_window_title();
                self.update_status_bar();
                Ok(())
            }
            Err(e) => {
                let msg = format!("Cannot open file '{}': {}", path.display(), e);
                self.show_error(&msg);
                Err(msg)
            }
        }
    }

    /// Executes `IDM_FILESAVE` (`0x03` -> `0x14000DC73` -> `SaveFile` at `0x140011AF8`).
    pub fn cmd_file_save(&mut self) -> bool {
        if let Some(path) = self.current_file.clone() {
            self.save_to_path(path)
        } else {
            self.cmd_file_save_as()
        }
    }

    /// Executes `IDM_FILESAVEAS` (`0x04` -> `0x14000DCAF` -> `GetSaveFileNameW` at `0x14000D45D`).
    pub fn cmd_file_save_as(&mut self) -> bool {
        let mut dlg = SaveFileDialog::new(self.hwnd_main);
        let default_path = self
            .current_file
            .clone()
            .unwrap_or_else(|| PathBuf::from("*.txt"));
        dlg = dlg.initial_file(&default_path);

        match dlg.show() {
            Ok(Some(path)) => self.save_to_path(path),
            Ok(None) => false,
            Err(err) => {
                self.show_error(&format!("Save dialog error: {}", err));
                false
            }
        }
    }

    fn save_to_path(&mut self, path: PathBuf) -> bool {
        let text = get_edit_text(self.hwnd_edit);
        match save_file_win32(&path, &text, self.encoding) {
            Ok(()) => {
                self.current_file = Some(path);
                set_edit_modified(self.hwnd_edit, false);
                self.update_window_title();
                self.update_status_bar();
                true
            }
            Err(e) => {
                self.show_error(&format!("Cannot save file '{}': {}", path.display(), e));
                false
            }
        }
    }

    /// Inserts current time and date (`InsertDateTime` at `0x140010580`, called on `ID_EDIT_TIMEDATE` = `0x1A`).
    pub fn insert_date_time(&mut self, leading_newline: bool) {
        let st = unsafe { GetLocalTime() };
        let stamp_clean = if leading_newline {
            format!(
                "\r\n{:02}:{:02} {:02}/{:02}/{:04}\r\n",
                st.wHour, st.wMinute, st.wMonth, st.wDay, st.wYear
            )
        } else {
            format!(
                "{:02}:{:02} {:02}/{:02}/{:04}",
                st.wHour, st.wMinute, st.wMonth, st.wDay, st.wYear
            )
        };

        if self.multi_cursor.cursors.is_multi() || self.multi_cursor.cursors.has_block_selection() {
            self.multi_cursor.insert_text(&stamp_clean);
            self.sync_to_hwnd_edit(true);
            return;
        }

        let mut stamp_nul = stamp_clean;
        stamp_nul.push('\0');
        let wide: Vec<u16> = stamp_nul.encode_utf16().collect();
        unsafe {
            SendMessageW(
                self.hwnd_edit,
                EM_REPLACESEL,
                Some(WPARAM(1)),
                Some(LPARAM(wide.as_ptr() as isize)),
            );
        }
        self.sync_from_hwnd_edit();
    }

    // =========================================================================
    // Multi-Cursor & Block Selection Command Handlers
    // =========================================================================

    /// Adds a cursor directly above the topmost cursor (`Ctrl+Alt+Up`).
    pub fn cmd_add_cursor_above(&mut self) {
        self.sync_from_hwnd_edit();
        if self.multi_cursor.add_cursor_above().is_some() {
            self.sync_to_hwnd_edit(false);
        }
    }

    /// Adds a cursor directly below the bottommost cursor (`Ctrl+Alt+Down`).
    pub fn cmd_add_cursor_below(&mut self) {
        self.sync_from_hwnd_edit();
        if self.multi_cursor.add_cursor_below().is_some() {
            self.sync_to_hwnd_edit(false);
        }
    }

    /// Selects the word under the cursor or adds a cursor at the next match (`Ctrl+D`).
    pub fn cmd_add_cursor_next_match(&mut self) {
        self.sync_from_hwnd_edit();
        if self.multi_cursor.add_cursor_at_next_match().is_some() {
            self.sync_to_hwnd_edit(false);
        }
    }

    /// Splits the current multi-line selection into cursors at the end of each line (`Alt+Shift+I`).
    pub fn cmd_add_cursors_to_line_ends(&mut self) {
        self.sync_from_hwnd_edit();
        self.multi_cursor.add_cursors_to_line_ends();
        self.sync_to_hwnd_edit(false);
    }

    /// Collapses secondary cursors and clears block selection (`Esc`).
    pub fn cmd_collapse_cursors(&mut self) {
        self.multi_cursor.collapse_to_primary_cursor();
        self.sync_to_hwnd_edit(false);
    }

    /// Extends a rectangular block selection in `direction` (`Shift+Alt+Arrows`).
    pub fn cmd_block_select(&mut self, direction: BlockDirection) {
        self.sync_from_hwnd_edit();
        self.multi_cursor.block_select(direction);
        self.sync_to_hwnd_edit(false);
    }

    /// Moves all active cursors in `direction` (`Arrows` / `Home` / `End`).
    pub fn cmd_multi_move(&mut self, direction: MoveDirection, extend_selection: bool) {
        self.multi_cursor.move_cursors(direction, extend_selection);
        self.sync_to_hwnd_edit(false);
    }

    /// Inserts a character or newline across all active cursors or block selection.
    pub fn cmd_multi_insert_char(&mut self, ch: char) {
        let insert_str = if ch == '\r' || ch == '\n' {
            "\r\n".to_string()
        } else {
            ch.to_string()
        };
        self.multi_cursor.insert_text(&insert_str);
        self.sync_to_hwnd_edit(true);
    }

    /// Performs `Backspace` across all active cursors or block selection.
    pub fn cmd_multi_backspace(&mut self) {
        self.multi_cursor.delete_backward();
        self.sync_to_hwnd_edit(true);
    }

    /// Performs `Delete` across all active cursors or block selection.
    pub fn cmd_multi_delete(&mut self) {
        self.multi_cursor.delete_forward();
        self.sync_to_hwnd_edit(true);
    }

    /// Renders multiple carets, secondary stream selections, and rectangular block selections
    /// onto `hwnd_edit` after its native `WM_PAINT` completes.
    pub unsafe fn render_multi_cursor_and_block_overlays(&self) {
        if self.hwnd_edit.is_invalid() {
            return;
        }
        if !self.multi_cursor.cursors.is_multi()
            && !self.multi_cursor.cursors.has_block_selection()
        {
            return;
        }

        let hdc = GetDC(Some(self.hwnd_edit));
        if hdc.is_invalid() {
            return;
        }

        // Select the Edit control's active font into the DC so TEXTMETRICW matches exactly
        let hfont = SendMessageW(self.hwnd_edit, WM_GETFONT, None, None);
        let old_font = if hfont.0 != 0 {
            SelectObject(hdc, HGDIOBJ(hfont.0 as *mut core::ffi::c_void))
        } else {
            HGDIOBJ::default()
        };

        let mut tm = TEXTMETRICW::default();
        let _ = GetTextMetricsW(hdc, &mut tm);
        let line_height = tm.tmHeight.max(14);
        let ave_char_width = tm.tmAveCharWidth.max(7);

        let ctx = self.multi_cursor.selection_context();
        let line_ranges = self.multi_cursor.line_content_ranges();

        // 1. Render Rectangular Block Selections (`ctx.block_rects`)
        for &(start_line, start_col, end_line, end_col) in &ctx.block_rects {
            for line_idx in start_line..=end_line {
                if let Some(line_range) = line_ranges.get(line_idx) {
                    let line_str = &self.multi_cursor.text()[line_range.clone()];
                    let line_chars: Vec<char> = line_str.chars().collect();
                    let line_char_len = line_chars.len();

                    let actual_sc = start_col.min(line_char_len);
                    let actual_ec = end_col.min(line_char_len);
                    let virtual_sc = start_col.saturating_sub(line_char_len);
                    let virtual_ec = end_col.saturating_sub(line_char_len);

                    let sc_byte_rel: usize = line_chars[..actual_sc]
                        .iter()
                        .map(|c| c.len_utf8())
                        .sum();
                    let ec_byte_rel: usize = line_chars[..actual_ec]
                        .iter()
                        .map(|c| c.len_utf8())
                        .sum();

                    let (base_sc_x, line_y) =
                        self.char_byte_to_client_xy(line_range.start + sc_byte_rel, line_height);
                    let (base_ec_x, _) =
                        self.char_byte_to_client_xy(line_range.start + ec_byte_rel, line_height);

                    let x1 = base_sc_x + (virtual_sc as i32) * ave_char_width;
                    let x2 = base_ec_x + (virtual_ec as i32) * ave_char_width;

                    if x2 > x1 {
                        let rect = RECT {
                            left: x1,
                            top: line_y,
                            right: x2,
                            bottom: line_y + line_height,
                        };
                        let _ = InvertRect(hdc, &rect);
                    }

                    // Draw a per-line block edge caret bar at the active column using theme.caret
                    let active_col = self
                        .multi_cursor
                        .cursors
                        .primary()
                        .sticky_column
                        .unwrap_or(end_col);
                    let caret_x = if active_col <= start_col { x1 } else { x2 };
                    let caret_brush = CreateSolidBrush(self.theme.caret.to_colorref());
                    let caret_rect = RECT {
                        left: caret_x,
                        top: line_y,
                        right: caret_x + 2,
                        bottom: line_y + line_height,
                    };
                    let _ = FillRect(hdc, &caret_rect, caret_brush);
                    let _ = DeleteObject(HGDIOBJ(caret_brush.0));
                }
            }
        }

        // 2. Render Multi-Cursor Stream Selections (`ctx.ranges`)
        for range in &ctx.ranges {
            let start_2d = self.multi_cursor.byte_to_2d(range.start);
            let end_2d = self.multi_cursor.byte_to_2d(range.end);
            for line_idx in start_2d.line..=end_2d.line {
                if let Some(line_range) = line_ranges.get(line_idx) {
                    let seg_start = if line_idx == start_2d.line {
                        range.start
                    } else {
                        line_range.start
                    };
                    let seg_end = if line_idx == end_2d.line {
                        range.end
                    } else {
                        line_range.end
                    };
                    if seg_end > seg_start {
                        let (x1, y1) = self.char_byte_to_client_xy(seg_start, line_height);
                        let (x2, _) = self.char_byte_to_client_xy(seg_end, line_height);
                        if x2 > x1 {
                            let rect = RECT {
                                left: x1,
                                top: y1,
                                right: x2,
                                bottom: y1 + line_height,
                            };
                            let _ = InvertRect(hdc, &rect);
                        }
                    }
                }
            }
        }

        // 3. Render Multiple Carets (`ctx.cursor_positions`) using theme.caret & theme.secondary_caret
        if !self.multi_cursor.cursors.has_block_selection() {
            let primary_brush = CreateSolidBrush(self.theme.caret.to_colorref());
            let secondary_brush = CreateSolidBrush(self.theme.secondary_caret.to_colorref());

            for &byte_pos in &ctx.cursor_positions {
                let (x, y) = self.char_byte_to_client_xy(byte_pos, line_height);
                let caret_rect = RECT {
                    left: x,
                    top: y,
                    right: x + 2,
                    bottom: y + line_height,
                };
                let brush = if byte_pos == ctx.primary_cursor_position {
                    primary_brush
                } else {
                    secondary_brush
                };
                let _ = FillRect(hdc, &caret_rect, brush);
            }

            let _ = DeleteObject(HGDIOBJ(primary_brush.0));
            let _ = DeleteObject(HGDIOBJ(secondary_brush.0));
        }

        if !old_font.is_invalid() {
            let _ = SelectObject(hdc, old_font);
        }
        let _ = ReleaseDC(Some(self.hwnd_edit), hdc);
    }

    /// Renders the left line-number gutter into `hdc` on `self.hwnd_main`, completely separated
    /// from the main text-buffer rendering in `self.hwnd_edit` (modeled after Sublime Text's
    /// gutter pass and `ColorScheme` offsets `[r12 + 0x46C..0x48C]`).
    pub unsafe fn paint_gutter_hdc(&self, hdc: HDC) {
        if self.hwnd_main.is_invalid()
            || !self.line_numbers_visible
            || self.gutter_layout.total_width <= 0
            || hdc.is_invalid()
        {
            return;
        }

        let mut rc = RECT::default();
        let _ = GetClientRect(self.hwnd_main, &mut rc);
        let client_h = (rc.bottom - rc.top).max(0);
        let edit_h = (client_h - self.active_status_height()).max(0);
        let gutter_w = self.gutter_layout.total_width;
        if edit_h <= 0 || gutter_w <= 0 {
            return;
        }

        // 1. Fill gutter background strip `[0, 0, gutter_w, edit_h]` with `theme.gutter` (`[r12 + 0x46C]`)
        let gutter_bg_brush = CreateSolidBrush(self.theme.gutter.to_colorref());
        let gutter_rect = RECT {
            left: 0,
            top: 0,
            right: gutter_w,
            bottom: edit_h,
        };
        let _ = FillRect(hdc, &gutter_rect, gutter_bg_brush);
        let _ = DeleteObject(HGDIOBJ(gutter_bg_brush.0));

        // 2. Draw 1px vertical separator border at `x = gutter_w - 1` using `theme.selection_border` (`[r12 + 0x400]`)
        let border_brush = CreateSolidBrush(self.theme.selection_border.to_colorref());
        let border_rect = RECT {
            left: (gutter_w - 1).max(0),
            top: 0,
            right: gutter_w,
            bottom: edit_h,
        };
        let _ = FillRect(hdc, &border_rect, border_brush);
        let _ = DeleteObject(HGDIOBJ(border_brush.0));

        // 3. Collect all 0-based line indices that contain an active cursor (`highlight_line_number` `[rsi + 0x56C]`)
        let mut active_lines = HashSet::new();
        for (_, cursor) in self.multi_cursor.cursors.iter() {
            let pos_2d = self.multi_cursor.byte_to_2d(cursor.position);
            active_lines.insert(pos_2d.line);
        }

        // 4. Select editor font and configure transparent background mode
        let hfont = if !self.hwnd_edit.is_invalid() {
            SendMessageW(self.hwnd_edit, WM_GETFONT, None, None)
        } else {
            LRESULT(0)
        };
        let old_font = if hfont.0 != 0 {
            SelectObject(hdc, HGDIOBJ(hfont.0 as *mut core::ffi::c_void))
        } else {
            let stock = GetStockObject(DEFAULT_GUI_FONT);
            if !stock.is_invalid() {
                SelectObject(hdc, stock)
            } else {
                HGDIOBJ::default()
            }
        };

        let mut tm = TEXTMETRICW::default();
        let _ = GetTextMetricsW(hdc, &mut tm);
        let line_height = tm.tmHeight.max(14);
        let _ = SetBkMode(hdc, TRANSPARENT);

        // 5. Query the first visible line in `hwnd_edit` (`EM_GETFIRSTVISIBLELINE = 0x00CE`)
        let first_visible = if !self.hwnd_edit.is_invalid() {
            SendMessageW(self.hwnd_edit, EM_GETFIRSTVISIBLELINE, None, None)
                .0
                .max(0) as usize
        } else {
            0
        };

        let line_ranges = self.multi_cursor.line_content_ranges();
        let total_lines = line_ranges.len().max(1);
        let highlight_brush = CreateSolidBrush(self.theme.line_highlight.to_colorref());

        // 6. Iterate through visible lines and draw right-aligned 1-based line numbers
        for line_idx in first_visible..total_lines {
            let y = if let Some(range) = line_ranges.get(line_idx) {
                let (_, char_y) = self.char_byte_to_client_xy(range.start, line_height);
                char_y
            } else {
                ((line_idx.saturating_sub(first_visible)) as i32) * line_height
            };

            if y >= edit_h {
                break;
            }
            if y + line_height <= 0 {
                continue;
            }

            let is_active = active_lines.contains(&line_idx);
            if is_active {
                // Highlight active line's gutter cell (`highlight_gutter` `[rsi + 0x56B]`)
                let row_rect = RECT {
                    left: 0,
                    top: y.max(0),
                    right: (gutter_w - 1).max(0),
                    bottom: (y + line_height).min(edit_h),
                };
                let _ = FillRect(hdc, &row_rect, highlight_brush);
                let _ = SetTextColor(hdc, self.theme.gutter_foreground_highlight.to_colorref());
            } else {
                let _ = SetTextColor(hdc, self.theme.gutter_foreground.to_colorref());
            }

            let formatted_num = format!(
                "{:>width$}",
                line_idx + 1,
                width = self.gutter_layout.digit_count
            );
            let wide_num: Vec<u16> = formatted_num.encode_utf16().collect();
            let _ = TextOutW(
                hdc,
                self.gutter_layout.horizontal_margin,
                y,
                &wide_num,
            );
        }

        let _ = DeleteObject(HGDIOBJ(highlight_brush.0));
        if !old_font.is_invalid() {
            let _ = SelectObject(hdc, old_font);
        }
    }

    /// Immediately repaints the left line-number gutter on `self.hwnd_main`.
    pub unsafe fn render_gutter_direct(&self) {
        if self.hwnd_main.is_invalid() || !self.line_numbers_visible {
            return;
        }
        let hdc = GetDC(Some(self.hwnd_main));
        if !hdc.is_invalid() {
            self.paint_gutter_hdc(hdc);
            let _ = ReleaseDC(Some(self.hwnd_main), hdc);
        }
    }

    /// Computes client-area `(x, y)` pixel coordinates for a UTF-8 byte offset in `hwnd_edit`.
    unsafe fn char_byte_to_client_xy(&self, byte_pos: usize, line_height: i32) -> (i32, i32) {
        let text = self.multi_cursor.text();
        let u16_idx = byte_to_utf16_offset(text, byte_pos);
        let pos_res = SendMessageW(
            self.hwnd_edit,
            EM_POSFROMCHAR,
            Some(WPARAM(u16_idx)),
            Some(LPARAM(0)),
        )
        .0;

        if pos_res != -1 {
            let x = (pos_res & 0xFFFF) as i16 as i32;
            let y = ((pos_res >> 16) & 0xFFFF) as i16 as i32;
            return (x, y);
        }

        // Fallback when `u16_idx` is at the very end of the buffer (where EM_POSFROMCHAR may return -1)
        if text.is_empty() || byte_pos == 0 {
            let zero_res = SendMessageW(
                self.hwnd_edit,
                EM_POSFROMCHAR,
                Some(WPARAM(0)),
                Some(LPARAM(0)),
            )
            .0;
            if zero_res != -1 {
                let x = (zero_res & 0xFFFF) as i16 as i32;
                let y = ((zero_res >> 16) & 0xFFFF) as i16 as i32;
                return (x, y);
            }
            return (2, 2);
        }

        // If the buffer ends with a newline, position at the start of the next line
        if text[..byte_pos.min(text.len())].ends_with('\n') {
            let prev_u16 = u16_idx.saturating_sub(1);
            let prev_res = SendMessageW(
                self.hwnd_edit,
                EM_POSFROMCHAR,
                Some(WPARAM(prev_u16)),
                Some(LPARAM(0)),
            )
            .0;
            let base_y = if prev_res != -1 {
                ((prev_res >> 16) & 0xFFFF) as i16 as i32 + line_height
            } else {
                2
            };
            return (2, base_y);
        }

        // Otherwise measure from the previous character and advance by 1 character width
        let prev_u16 = u16_idx.saturating_sub(1);
        let prev_res = SendMessageW(
            self.hwnd_edit,
            EM_POSFROMCHAR,
            Some(WPARAM(prev_u16)),
            Some(LPARAM(0)),
        )
        .0;
        if prev_res != -1 {
            let x = (prev_res & 0xFFFF) as i16 as i32 + 8;
            let y = ((prev_res >> 16) & 0xFFFF) as i16 as i32;
            return (x, y);
        }

        (2, 2)
    }

    fn show_error(&self, message: &str) {
        let wide_msg: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
        let wide_cap: Vec<u16> = "Notepad\0".encode_utf16().collect();
        unsafe {
            let _ = MessageBoxW(
                Some(self.hwnd_main),
                PCWSTR(wide_msg.as_ptr()),
                PCWSTR(wide_cap.as_ptr()),
                MB_OK | MB_ICONWARNING,
            );
        }
    }
}

/// Attaches the multi-cursor/block-selection subclass procedure to `hwnd_edit`.
pub unsafe fn attach_edit_subclass(hwnd_edit: HWND, state_ptr: *mut NotepadAppState) {
    if hwnd_edit.is_invalid() || state_ptr.is_null() {
        return;
    }
    SetWindowLongPtrW(hwnd_edit, GWLP_USERDATA, state_ptr as isize);
    let prev = SetWindowLongPtrW(
        hwnd_edit,
        GWLP_WNDPROC,
        edit_subclass_proc as *const () as usize as isize,
    );
    if let Some(state) = state_ptr.as_mut() {
        state.edit_prev_wndproc = prev;
    }
}

/// Detaches the subclass procedure from `hwnd_edit` before window destruction so no
/// trailing messages (`WM_DESTROY` / `WM_NCDESTROY`) dereference `GWLP_USERDATA`.
pub unsafe fn detach_edit_subclass(hwnd_edit: HWND, prev_wndproc: isize) {
    if hwnd_edit.is_invalid() {
        return;
    }
    SetWindowLongPtrW(hwnd_edit, GWLP_USERDATA, 0);
    if prev_wndproc != 0 {
        SetWindowLongPtrW(hwnd_edit, GWLP_WNDPROC, prev_wndproc);
    }
}

/// Converts client `(x, y)` mouse coordinates in `hwnd_edit` to a UTF-8 byte offset.
unsafe fn mouse_lparam_to_byte_offset(state: &NotepadAppState, lparam: LPARAM) -> usize {
    let char_res = SendMessageW(
        state.hwnd_edit,
        EM_CHARFROMPOS,
        Some(WPARAM(0)),
        Some(lparam),
    )
    .0;
    let u16_index = (char_res & 0xFFFF) as usize;
    utf16_to_byte_offset(state.multi_cursor.text(), u16_index)
}

/// Subclass Window Procedure for the child `"Edit"` control.
/// Intercepts multi-cursor and rectangular block-selection mouse/keyboard inputs
/// and renders custom GDI carets and selection blocks on `WM_PAINT`.
pub unsafe extern "system" fn edit_subclass_proc(
    hwnd_edit: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = GetWindowLongPtrW(hwnd_edit, GWLP_USERDATA) as *mut NotepadAppState;
    let Some(state) = state_ptr.as_mut() else {
        return DefWindowProcW(hwnd_edit, msg, wparam, lparam);
    };

    let prev_proc: WNDPROC = if state.edit_prev_wndproc != 0 {
        Some(std::mem::transmute::<
            isize,
            unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
        >(state.edit_prev_wndproc))
    } else {
        None
    };

    let ctrl_down = GetKeyState(VK_CONTROL.0 as i32) < 0;
    let alt_down = GetKeyState(VK_MENU.0 as i32) < 0 || ((lparam.0 >> 29) & 1) != 0;
    let shift_down = GetKeyState(VK_SHIFT.0 as i32) < 0;

    match msg {
        WM_LBUTTONDOWN => {
            let _ = SetFocus(Some(hwnd_edit));
            if alt_down {
                state.sync_from_hwnd_edit();
                let click_byte = mouse_lparam_to_byte_offset(state, lparam);
                state.alt_drag_origin = Some(click_byte);
                state.multi_cursor.handle_alt_click(click_byte);
                state.sync_to_hwnd_edit(false);
                return LRESULT(0);
            } else {
                state.alt_drag_origin = None;
                if state.multi_cursor.cursors.is_multi()
                    || state.multi_cursor.cursors.has_block_selection()
                {
                    state.multi_cursor.collapse_to_primary_cursor();
                    let _ = InvalidateRect(Some(hwnd_edit), None, false);
                }
            }
        }

        WM_MOUSEMOVE => {
            let lbutton_down = (wparam.0 & MK_LBUTTON_MASK) != 0;
            if lbutton_down && alt_down {
                if let Some(origin_byte) = state.alt_drag_origin {
                    let current_byte = mouse_lparam_to_byte_offset(state, lparam);
                    if current_byte != origin_byte
                        || state.multi_cursor.cursors.has_block_selection()
                    {
                        state.multi_cursor.handle_alt_drag(origin_byte, current_byte);
                        state.sync_to_hwnd_edit(false);
                        return LRESULT(0);
                    }
                }
            }
        }

        WM_LBUTTONUP => {
            state.alt_drag_origin = None;
            let res = CallWindowProcW(prev_proc, hwnd_edit, msg, wparam, lparam);
            state.sync_from_hwnd_edit();
            state.update_status_bar();
            return res;
        }

        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let vk = wparam.0 as u16;

            // 1. Ctrl+Alt+Up / Ctrl+Alt+Down -> Add cursor above / below
            if ctrl_down && alt_down && !shift_down {
                if vk == VK_UP.0 {
                    state.cmd_add_cursor_above();
                    return LRESULT(0);
                }
                if vk == VK_DOWN.0 {
                    state.cmd_add_cursor_below();
                    return LRESULT(0);
                }
            }

            // 2. Ctrl+D -> Add cursor at next match of selection (or select word under cursor)
            if ctrl_down && !alt_down && !shift_down && vk == b'D' as u16 {
                state.cmd_add_cursor_next_match();
                return LRESULT(0);
            }

            // 3. Alt+Shift+I -> Add cursors to line ends of selection
            if alt_down && shift_down && !ctrl_down && vk == b'I' as u16 {
                state.cmd_add_cursors_to_line_ends();
                return LRESULT(0);
            }

            // 4. Shift+Alt+Arrows -> Rectangular block selection (Column Selection)
            if shift_down && alt_down && !ctrl_down {
                let block_dir = match vk {
                    k if k == VK_LEFT.0 => Some(BlockDirection::Left),
                    k if k == VK_RIGHT.0 => Some(BlockDirection::Right),
                    k if k == VK_UP.0 => Some(BlockDirection::Up),
                    k if k == VK_DOWN.0 => Some(BlockDirection::Down),
                    _ => None,
                };
                if let Some(dir) = block_dir {
                    state.cmd_block_select(dir);
                    return LRESULT(0);
                }
            }

            // 5. Escape -> Collapse secondary cursors and clear rectangular block selection
            if vk == VK_ESCAPE.0
                && (state.multi_cursor.cursors.is_multi()
                    || state.multi_cursor.cursors.has_block_selection())
            {
                state.cmd_collapse_cursors();
                return LRESULT(0);
            }

            // 6. Multi-Cursor & Block Selection Navigation / Deletion
            if state.multi_cursor.cursors.is_multi()
                || state.multi_cursor.cursors.has_block_selection()
            {
                if vk == VK_BACK.0 && !ctrl_down && !alt_down {
                    state.cmd_multi_backspace();
                    return LRESULT(0);
                }
                if vk == VK_DELETE.0 && !ctrl_down && !alt_down {
                    state.cmd_multi_delete();
                    return LRESULT(0);
                }

                if !alt_down && !ctrl_down {
                    let move_dir = match vk {
                        k if k == VK_LEFT.0 => Some(MoveDirection::Left),
                        k if k == VK_RIGHT.0 => Some(MoveDirection::Right),
                        k if k == VK_UP.0 => Some(MoveDirection::Up),
                        k if k == VK_DOWN.0 => Some(MoveDirection::Down),
                        k if k == VK_HOME.0 => Some(MoveDirection::Home),
                        k if k == VK_END.0 => Some(MoveDirection::End),
                        _ => None,
                    };
                    if let Some(dir) = move_dir {
                        state.cmd_multi_move(dir, shift_down);
                        return LRESULT(0);
                    }
                }
            } else {
                // Single-cursor navigation updates the status bar and active gutter line after standard processing
                let is_nav = matches!(
                    vk,
                    k if k == VK_LEFT.0
                        || k == VK_RIGHT.0
                        || k == VK_UP.0
                        || k == VK_DOWN.0
                        || k == VK_HOME.0
                        || k == VK_END.0
                        || k == VK_PRIOR.0
                        || k == VK_NEXT.0
                );
                if is_nav {
                    let res = CallWindowProcW(prev_proc, hwnd_edit, msg, wparam, lparam);
                    state.sync_from_hwnd_edit();
                    state.update_status_bar();
                    return res;
                }
            }
        }

        WM_VSCROLL | WM_MOUSEWHEEL => {
            let res = CallWindowProcW(prev_proc, hwnd_edit, msg, wparam, lparam);
            state.render_gutter_direct();
            return res;
        }

        WM_SYSCHAR => {
            // Prevent system menu beep on Alt+Shift+I or Alt+Shift+Arrows
            let ch_code = (wparam.0 & 0xFFFF) as u16;
            if ch_code == b'I' as u16 || ch_code == b'i' as u16 || shift_down {
                return LRESULT(0);
            }
        }

        WM_CHAR => {
            let ch_u32 = (wparam.0 & 0xFFFF) as u32;
            // Suppress Ctrl+D control char (0x04 = EOT)
            if ch_u32 == 0x04 {
                return LRESULT(0);
            }

            if state.multi_cursor.cursors.is_multi()
                || state.multi_cursor.cursors.has_block_selection()
            {
                if let Some(ch) = char::from_u32(ch_u32) {
                    if ch >= ' ' || ch == '\t' || ch == '\r' {
                        state.cmd_multi_insert_char(ch);
                        return LRESULT(0);
                    }
                }
            }
        }

        WM_PAINT => {
            let res = CallWindowProcW(prev_proc, hwnd_edit, msg, wparam, lparam);
            state.render_multi_cursor_and_block_overlays();
            state.render_gutter_direct();
            return res;
        }

        _ => {}
    }

    CallWindowProcW(prev_proc, hwnd_edit, msg, wparam, lparam)
}

/// Registers the `"Notepad"` window class (`NPRegister` at `0x14001E200` - `0x14001E2A0`).
pub fn register_notepad_class(hinstance: HINSTANCE) -> Result<u16, &'static str> {
    let class_name: Vec<u16> = NOTEPAD_CLASS_NAME.encode_utf16().collect();

    unsafe {
        // 0x14001E20E: LoadCursorW(NULL, IDC_IBEAM = 0x7F01)
        let hcursor = LoadCursorW(None, IDC_IBEAM).unwrap_or_default();
        // 0x14001E226: LoadIconW
        let hicon = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();

        let wcx = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32, // 0x50 at 0x14001E205
            style: Default::default(),
            lpfnWndProc: Some(notepad_wnd_proc), // 0x14000E950 at 0x14001E278
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: hicon,
            hCursor: hcursor,
            // 0x14001E287: mov qword ptr [rbp - 0x20], 6 (COLOR_WINDOW + 1)
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as *mut core::ffi::c_void),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: PCWSTR(class_name.as_ptr()), // 0x14002B828 ("Notepad")
            hIconSm: hicon,
        };

        let atom = RegisterClassExW(&wcx);
        if atom == 0 {
            return Err("RegisterClassExW failed");
        }
        Ok(atom)
    }
}

/// Creates the main `"Notepad"` window, child `"Edit"` control, and status bar (`NPInit` at `0x140013488`).
pub fn create_notepad_window(
    hinstance: HINSTANCE,
    show_cmd: Option<windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD>,
) -> Result<HWND, &'static str> {
    let _ = register_notepad_class(hinstance);
    let hmenu = create_notepad_menu()?;

    let class_name: Vec<u16> = NOTEPAD_CLASS_NAME.encode_utf16().collect();
    let title: Vec<u16> = "Untitled - Notepad\0".encode_utf16().collect();

    // 1. Create Main Window (0x1400134DC: WS_OVERLAPPEDWINDOW = 0x00CF0000)
    let hwnd_main = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            800,
            600,
            None,
            Some(hmenu),
            Some(hinstance),
            None,
        )
        .map_err(|_| "CreateWindowExW for main Notepad window failed")?
    };

    // 2. Measure initial client area (0x14001363D: GetClientRect)
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd_main, &mut rc);
    }
    let client_w = (rc.right - rc.left).max(0);
    let client_h = (rc.bottom - rc.top).max(0);
    let edit_h = (client_h - STATUS_BAR_HEIGHT).max(0);

    // 3. Create Child "Edit" Control (0x1400125E8)
    let initial_wrap = WordWrap::Disabled;
    let hwnd_edit = create_edit_control(hwnd_main, hinstance, client_w, edit_h, initial_wrap)?;

    // 4. Create Child Status Bar (0x140013715)
    let status_class: Vec<u16> = "STATIC\0".encode_utf16().collect();
    let status_text: Vec<u16> =
        "  Ln 1, Col 1   |   100%   |   Windows (CRLF)   |   UTF-8\0"
            .encode_utf16()
            .collect();
    let hwnd_status = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(status_class.as_ptr()),
            PCWSTR(status_text.as_ptr()),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
            0,
            edit_h,
            client_w,
            STATUS_BAR_HEIGHT,
            Some(hwnd_main),
            Some(HMENU(ID_STATUS_BAR as *mut core::ffi::c_void)),
            Some(hinstance),
            None,
        )
        .unwrap_or_default()
    };

    // Apply default GUI font and expand text limit on Edit control
    unsafe {
        let hfont = GetStockObject(DEFAULT_GUI_FONT);
        if !hfont.is_invalid() {
            SendMessageW(
                hwnd_edit,
                WM_SETFONT,
                Some(WPARAM(hfont.0 as usize)),
                Some(LPARAM(1)),
            );
            if !hwnd_status.is_invalid() {
                SendMessageW(
                    hwnd_status,
                    WM_SETFONT,
                    Some(WPARAM(hfont.0 as usize)),
                    Some(LPARAM(1)),
                );
            }
        }
        SendMessageW(
            hwnd_edit,
            EM_LIMITTEXT,
            Some(WPARAM(0x7FFF_FFFE)),
            Some(LPARAM(0)),
        );
    }

    // 5. Box application state with Sublime Mariana Dark theme & left line-number gutter enabled
    let initial_preset = ThemePreset::MarianaDark;
    let initial_theme = Theme::from_preset(initial_preset);
    let (edit_bg_brush, status_bg_brush) = unsafe {
        (
            CreateSolidBrush(initial_theme.background.to_colorref()),
            CreateSolidBrush(initial_theme.status_background.to_colorref()),
        )
    };
    let initial_gutter = GutterLayout::compute(1, 8, GutterLayout::DEFAULT_MARGIN_PX, true);

    let state = Box::new(NotepadAppState {
        hinstance,
        hwnd_main,
        hwnd_edit,
        hwnd_status,
        word_wrap: WordWrapManager::new(initial_wrap),
        status_bar_visible: true,
        current_file: None,
        encoding: TextEncoding::Utf8,
        multi_cursor: MultiCursorEditor::new(),
        edit_prev_wndproc: 0,
        alt_drag_origin: None,
        internal_clipboard: None,
        syncing_to_hwnd: false,
        theme_preset: initial_preset,
        theme: initial_theme,
        edit_bg_brush,
        status_bg_brush,
        line_numbers_visible: true,
        gutter_layout: initial_gutter,
    });
    let state_ptr = Box::into_raw(state);
    unsafe {
        SetWindowLongPtrW(hwnd_main, GWLP_USERDATA, state_ptr as isize);
        attach_edit_subclass(hwnd_edit, state_ptr);
        if let Some(state_ref) = state_ptr.as_mut() {
            state_ref.relayout_viewport();
            state_ref.update_status_bar();
        }
        if let Some(cmd) = show_cmd {
            let _ = ShowWindow(hwnd_main, cmd);
        }
        let _ = SetFocus(Some(hwnd_edit));
    }

    Ok(hwnd_main)
}

/// Main Window Procedure (`NPWndProc` at `0x14000E950`).
pub unsafe extern "system" fn notepad_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut NotepadAppState;

    if let Some(state) = state_ptr.as_mut() {
        match msg {
            // 0x14000EA6E: WM_SIZE (0x0005) -> NPSize (0x14000E848)
            WM_SIZE => {
                // wparam == 1 is SIZE_MINIMIZED (0x14000EA73: sub rdi, 1; je 0x14000ea96)
                if wparam.0 != 1 {
                    let width = (lparam.0 & 0xFFFF) as i16 as i32;
                    let height = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                    state.resize_children(width, height);
                }
                return LRESULT(0);
            }

            // 0x14000E9E5: WM_SETFOCUS (0x0007)
            WM_SETFOCUS => {
                if !IsIconic(hwnd).as_bool() && !state.hwnd_edit.is_invalid() {
                    let _ = SetFocus(Some(state.hwnd_edit));
                }
                return LRESULT(0);
            }

            // Sublime-inspired left line-number gutter rendering pass (WM_PAINT on main window)
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                if !hdc.is_invalid() {
                    state.paint_gutter_hdc(hdc);
                }
                let _ = EndPaint(hwnd, &ps);
                return LRESULT(0);
            }

            WM_ERASEBKGND => {
                return LRESULT(1);
            }

            // Theme coloring for the child "Edit" control (`[r12 + 0x308]` fg & `[r12 + 0x318]` bg)
            WM_CTLCOLOREDIT => {
                let hdc = HDC(wparam.0 as *mut core::ffi::c_void);
                if !hdc.is_invalid() {
                    let _ = SetTextColor(hdc, state.theme.foreground.to_colorref());
                    let _ = SetBkColor(hdc, state.theme.background.to_colorref());
                }
                return LRESULT(state.edit_bg_brush.0 as isize);
            }

            // Theme coloring for the child Status Bar ("STATIC" control)
            WM_CTLCOLORSTATIC => {
                let hdc = HDC(wparam.0 as *mut core::ffi::c_void);
                if !hdc.is_invalid() {
                    let _ = SetTextColor(hdc, state.theme.status_foreground.to_colorref());
                    let _ = SetBkColor(hdc, state.theme.status_background.to_colorref());
                }
                return LRESULT(state.status_bg_brush.0 as isize);
            }

            // 0x14000ED6D: WM_INITMENU (0x0116) -> InitMenu (0x14000F2A0)
            WM_INITMENU => {
                let editor_state = state.editor_state();
                let _ = init_menu(hwnd, state.hwnd_edit, &editor_state);
                return LRESULT(0);
            }

            // 0x14000EF8F: WM_COMMAND (0x0111) -> NPCommand (0x14000DA7C)
            WM_COMMAND => {
                let cmd_id = (wparam.0 & 0xFFFF) as u32;
                let notify_code = ((wparam.0 >> 16) & 0xFFFF) as u16;
                let child_hwnd = HWND(lparam.0 as *mut core::ffi::c_void);

                // 0x14000EF8F - 0x14000EFB6: Notification from g_hwndEdit
                if child_hwnd == state.hwnd_edit && !state.hwnd_edit.is_invalid() {
                    if notify_code == EN_CHANGE && !state.syncing_to_hwnd {
                        state.sync_from_hwnd_edit();
                        state.update_window_title();
                        state.update_status_bar();
                    }
                    return LRESULT(0);
                }

                // Dispatch menu/accelerator commands (NPCommand at 0x14000DA7C)
                match cmd_id {
                    IDM_FILENEW => state.cmd_file_new(),
                    IDM_FILEOPEN => state.cmd_file_open(),
                    IDM_FILESAVE => {
                        let _ = state.cmd_file_save();
                    }
                    IDM_FILESAVEAS => {
                        let _ = state.cmd_file_save_as();
                    }
                    IDM_FILEEXIT => {
                        // 0x14000DF4C: Post WM_CLOSE
                        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    ID_EDIT_UNDO => {
                        if state.multi_cursor.can_undo() {
                            if state.multi_cursor.undo() {
                                state.sync_to_hwnd_edit(true);
                            }
                        } else {
                            SendMessageW(state.hwnd_edit, EM_UNDO, None, None);
                            state.sync_from_hwnd_edit();
                        }
                    }
                    ID_EDIT_CUT => {
                        if state.multi_cursor.cursors.is_multi()
                            || state.multi_cursor.cursors.has_block_selection()
                        {
                            if let Some(cut_str) = state.multi_cursor.cut_selection() {
                                state.internal_clipboard = Some(cut_str);
                                state.sync_to_hwnd_edit(true);
                            }
                        } else {
                            SendMessageW(state.hwnd_edit, WM_CUT, None, None);
                            state.sync_from_hwnd_edit();
                        }
                    }
                    ID_EDIT_COPY => {
                        if state.multi_cursor.cursors.is_multi()
                            || state.multi_cursor.cursors.has_block_selection()
                        {
                            if let Some(copied) = state.multi_cursor.copy_selection() {
                                state.internal_clipboard = Some(copied);
                            }
                        } else {
                            state.sync_from_hwnd_edit();
                            state.internal_clipboard = state.multi_cursor.copy_selection();
                            SendMessageW(state.hwnd_edit, WM_COPY, None, None);
                        }
                    }
                    ID_EDIT_PASTE => {
                        if state.multi_cursor.cursors.is_multi()
                            || state.multi_cursor.cursors.has_block_selection()
                        {
                            if let Some(clip) = state.internal_clipboard.clone() {
                                state.multi_cursor.paste_text(&clip);
                                state.sync_to_hwnd_edit(true);
                            } else {
                                SendMessageW(state.hwnd_edit, WM_PASTE, None, None);
                                state.sync_from_hwnd_edit();
                            }
                        } else {
                            SendMessageW(state.hwnd_edit, WM_PASTE, None, None);
                            state.sync_from_hwnd_edit();
                        }
                    }
                    ID_EDIT_CLEAR => {
                        if state.multi_cursor.cursors.is_multi()
                            || state.multi_cursor.cursors.has_block_selection()
                        {
                            state.cmd_multi_delete();
                        } else {
                            SendMessageW(state.hwnd_edit, WM_CLEAR, None, None);
                            state.sync_from_hwnd_edit();
                        }
                    }
                    ID_EDIT_SELECTALL => {
                        state.multi_cursor.collapse_to_primary_cursor();
                        // 0x14000E3A0: SendMessageW(g_hwndEdit, EM_SETSEL, 0, -1) + EM_SCROLLCARET
                        SendMessageW(
                            state.hwnd_edit,
                            EM_SETSEL,
                            Some(WPARAM(0)),
                            Some(LPARAM(-1)),
                        );
                        SendMessageW(state.hwnd_edit, EM_SCROLLCARET, None, None);
                        state.sync_from_hwnd_edit();
                    }
                    ID_EDIT_TIMEDATE => {
                        // 0x14000E394: InsertDateTime (0x140010580)
                        state.insert_date_time(false);
                    }
                    ID_EDIT_ADD_CURSOR_ABOVE => {
                        state.cmd_add_cursor_above();
                    }
                    ID_EDIT_ADD_CURSOR_BELOW => {
                        state.cmd_add_cursor_below();
                    }
                    ID_EDIT_ADD_CURSOR_NEXT_MATCH => {
                        state.cmd_add_cursor_next_match();
                    }
                    ID_EDIT_ADD_CURSORS_LINE_ENDS => {
                        state.cmd_add_cursors_to_line_ends();
                    }
                    ID_EDIT_SINGLE_CURSOR => {
                        state.cmd_collapse_cursors();
                    }
                    ID_FORMAT_WORDWRAP => {
                        // 0x14000E1CE: Toggle Word Wrap & NPReCreate (0x14001FA28)
                        let status_h = state.active_status_height();
                        detach_edit_subclass(state.hwnd_edit, state.edit_prev_wndproc);
                        if let Ok(new_edit) = state.word_wrap.toggle_and_recreate_edit(
                            state.hwnd_main,
                            state.hwnd_edit,
                            state.hinstance,
                            status_h,
                        ) {
                            state.hwnd_edit = new_edit;
                            attach_edit_subclass(new_edit, state_ptr);
                            state.relayout_viewport();
                            state.sync_from_hwnd_edit();
                        }
                    }
                    ID_FORMAT_THEME_MARIANA => {
                        state.set_theme_preset(ThemePreset::MarianaDark);
                    }
                    ID_FORMAT_THEME_MONOKAI => {
                        state.set_theme_preset(ThemePreset::MonokaiDark);
                    }
                    ID_FORMAT_THEME_BREAKERS => {
                        state.set_theme_preset(ThemePreset::BreakersLight);
                    }
                    ID_FORMAT_THEME_CLASSIC => {
                        state.set_theme_preset(ThemePreset::ClassicNotepad);
                    }
                    ID_VIEW_LINE_NUMBERS => {
                        state.toggle_line_numbers();
                    }
                    ID_VIEW_STATUS => {
                        // 0x14000E30A: Toggle g_fStatusBar + ShowWindow + NPSize
                        state.status_bar_visible = !state.status_bar_visible;
                        if !state.hwnd_status.is_invalid() {
                            let _ = ShowWindow(
                                state.hwnd_status,
                                if state.status_bar_visible {
                                    SW_SHOW
                                } else {
                                    SW_HIDE
                                },
                            );
                        }
                        let mut rc = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rc);
                        state.resize_children(rc.right - rc.left, rc.bottom - rc.top);
                        state.update_status_bar();
                    }
                    IDM_HELP_ABOUT => {
                        let text: Vec<u16> =
                            "Reverse-Engineered Win32 Notepad Clone in Safe Rust (windows-rs v0.62)\0"
                                .encode_utf16()
                                .collect();
                        let cap: Vec<u16> = "About Notepad\0".encode_utf16().collect();
                        let _ = MessageBoxW(
                            Some(hwnd),
                            PCWSTR(text.as_ptr()),
                            PCWSTR(cap.as_ptr()),
                            MB_OK | MB_ICONINFORMATION,
                        );
                    }
                    _ => {}
                }
                return LRESULT(0);
            }

            // 0x14000EAC0: WM_CLOSE (0x0010) -> CheckSave + DestroyWindow
            WM_CLOSE => {
                if state.check_save() {
                    if !state.hwnd_edit.is_invalid() {
                        detach_edit_subclass(state.hwnd_edit, state.edit_prev_wndproc);
                    }
                    if !state.hwnd_status.is_invalid() {
                        let _ = DestroyWindow(state.hwnd_status);
                        state.hwnd_status = HWND::default();
                    }
                    let _ = DestroyWindow(hwnd);
                }
                return LRESULT(0);
            }

            // 0x14000EAAD: WM_DESTROY (0x0002) -> PostQuitMessage(0)
            WM_DESTROY => {
                if !state.hwnd_edit.is_invalid() {
                    detach_edit_subclass(state.hwnd_edit, state.edit_prev_wndproc);
                }
                if !state.edit_bg_brush.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ(state.edit_bg_brush.0));
                }
                if !state.status_bg_brush.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ(state.status_bg_brush.0));
                }
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let _ = Box::from_raw(state_ptr);
                PostQuitMessage(0);
                return LRESULT(0);
            }

            _ => {}
        }
    }

    DefWindowProcW(hwnd, msg, wparam, lparam)
}
