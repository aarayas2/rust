//! Reverse-engineered Notepad Window Procedure (`NPWndProc`), Class Registration (`NPRegister`),
//! Child Control Lifecycle (`NPInit` / `NPSize`), and Command Dispatch (`NPCommand`).
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

use crate::file_io::{
    get_edit_text, is_edit_modified, load_file_win32, save_file_win32, set_edit_modified,
    set_edit_text, TextEncoding,
};
use crate::menu::{
    create_notepad_menu, init_menu, EditorState, EM_GETSEL, EM_REPLACESEL, EM_SCROLLCARET,
    EM_SETSEL, EM_UNDO, IDM_FILEEXIT, IDM_FILENEW, IDM_FILEOPEN, IDM_FILESAVE, IDM_FILESAVEAS,
    IDM_HELP_ABOUT, ID_EDIT_CLEAR, ID_EDIT_COPY, ID_EDIT_CUT, ID_EDIT_PASTE, ID_EDIT_SELECTALL,
    ID_EDIT_TIMEDATE, ID_EDIT_UNDO, ID_VIEW_STATUS,
};
use crate::save_dialog::{OpenFileDialog, SaveFileDialog};
use crate::word_wrap::{
    create_edit_control, WordWrap, WordWrapManager, ID_FORMAT_WORDWRAP,
};
use std::path::PathBuf;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, COLOR_WINDOW, DEFAULT_GUI_FONT, HBRUSH};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW, IsIconic,
    LoadCursorW, LoadIconW, MessageBoxW, MoveWindow, PostMessageW, PostQuitMessage,
    RegisterClassExW, SendMessageW, SetWindowLongPtrW, SetWindowTextW, ShowWindow, CW_USEDEFAULT,
    GWLP_USERDATA, HMENU, IDC_IBEAM, IDI_APPLICATION, IDNO, IDYES, MB_ICONINFORMATION,
    MB_ICONWARNING, MB_OK, MB_YESNOCANCEL, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CLEAR, WM_CLOSE, WM_COMMAND, WM_COPY, WM_CUT, WM_DESTROY, WM_INITMENU, WM_PASTE,
    WM_SETFOCUS, WM_SETFONT, WM_SIZE, WNDCLASSEXW, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
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
}

impl NotepadAppState {
    /// Returns the current `EditorState` snapshot used by `init_menu` (`0x14000F2A0`).
    pub fn editor_state(&self) -> EditorState {
        EditorState {
            word_wrap: self.word_wrap.state(),
            status_bar_visible: self.status_bar_visible,
        }
    }

    /// Returns the active status bar height (`0` when hidden, `STATUS_BAR_HEIGHT` when visible).
    pub fn active_status_height(&self) -> i32 {
        if self.status_bar_visible {
            STATUS_BAR_HEIGHT
        } else {
            0
        }
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

    /// Updates the status bar text (`Ln X, Col Y | UTF-8 | Windows (CRLF)`).
    pub fn update_status_bar(&self) {
        if self.hwnd_status.is_invalid() || !self.status_bar_visible {
            return;
        }

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

        let enc_label = match self.encoding {
            TextEncoding::Utf8 => "UTF-8",
            TextEncoding::Utf8Bom => "UTF-8-BOM",
            TextEncoding::Utf16Le => "UTF-16 LE",
            TextEncoding::Utf16Be => "UTF-16 BE",
        };

        let status_str = format!(
            "  Ln {}, Col {}   |   100%   |   Windows (CRLF)   |   {}\0",
            line, col, enc_label
        );
        let wide: Vec<u16> = status_str.encode_utf16().collect();
        unsafe {
            let _ = SetWindowTextW(self.hwnd_status, PCWSTR(wide.as_ptr()));
        }
    }

    /// Resizes the child `"Edit"` control and status bar to fit the main window client area
    /// (`NPSize` at `0x14000E848` - `0x14000E93D`).
    pub fn resize_children(&self, width: i32, height: i32) {
        let status_h = self.active_status_height();
        let edit_h = (height - status_h).max(0);

        unsafe {
            if !self.hwnd_edit.is_invalid() {
                // 0x14000E8ED: MoveWindow(g_hwndEdit, 0, 0, cx, cy - statusHeight, TRUE)
                let _ = MoveWindow(self.hwnd_edit, 0, 0, width, edit_h, true);
            }
            if !self.hwnd_status.is_invalid() && self.status_bar_visible {
                // 0x14000E915: MoveWindow(g_hwndStatus, 0, cy - statusHeight, cx, statusHeight, TRUE)
                let _ = MoveWindow(self.hwnd_status, 0, edit_h, width, status_h, true);
            }
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
        self.current_file = None;
        self.encoding = TextEncoding::Utf8;
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
    pub fn insert_date_time(&self, leading_newline: bool) {
        let st = unsafe { GetLocalTime() };
        let stamp = if leading_newline {
            format!(
                "\r\n{:02}:{:02} {:02}/{:02}/{:04}\r\n\0",
                st.wHour, st.wMinute, st.wMonth, st.wDay, st.wYear
            )
        } else {
            format!(
                "{:02}:{:02} {:02}/{:02}/{:04}\0",
                st.wHour, st.wMinute, st.wMonth, st.wDay, st.wYear
            )
        };
        let wide: Vec<u16> = stamp.encode_utf16().collect();
        unsafe {
            SendMessageW(
                self.hwnd_edit,
                EM_REPLACESEL,
                Some(WPARAM(1)),
                Some(LPARAM(wide.as_ptr() as isize)),
            );
        }
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

    // 5. Box application state and attach to GWLP_USERDATA
    let state = Box::new(NotepadAppState {
        hinstance,
        hwnd_main,
        hwnd_edit,
        hwnd_status,
        word_wrap: WordWrapManager::new(initial_wrap),
        status_bar_visible: true,
        current_file: None,
        encoding: TextEncoding::Utf8,
    });
    let state_ptr = Box::into_raw(state);
    unsafe {
        SetWindowLongPtrW(hwnd_main, GWLP_USERDATA, state_ptr as isize);
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
                    if notify_code == EN_CHANGE {
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
                        SendMessageW(state.hwnd_edit, EM_UNDO, None, None);
                    }
                    ID_EDIT_CUT => {
                        SendMessageW(state.hwnd_edit, WM_CUT, None, None);
                    }
                    ID_EDIT_COPY => {
                        SendMessageW(state.hwnd_edit, WM_COPY, None, None);
                    }
                    ID_EDIT_PASTE => {
                        SendMessageW(state.hwnd_edit, WM_PASTE, None, None);
                    }
                    ID_EDIT_CLEAR => {
                        SendMessageW(state.hwnd_edit, WM_CLEAR, None, None);
                    }
                    ID_EDIT_SELECTALL => {
                        // 0x14000E3A0: SendMessageW(g_hwndEdit, EM_SETSEL, 0, -1) + EM_SCROLLCARET
                        SendMessageW(
                            state.hwnd_edit,
                            EM_SETSEL,
                            Some(WPARAM(0)),
                            Some(LPARAM(-1)),
                        );
                        SendMessageW(state.hwnd_edit, EM_SCROLLCARET, None, None);
                    }
                    ID_EDIT_TIMEDATE => {
                        // 0x14000E394: InsertDateTime (0x140010580)
                        state.insert_date_time(false);
                    }
                    ID_FORMAT_WORDWRAP => {
                        // 0x14000E1CE: Toggle Word Wrap & NPReCreate (0x14001FA28)
                        let status_h = state.active_status_height();
                        if let Ok(new_edit) = state.word_wrap.toggle_and_recreate_edit(
                            state.hwnd_main,
                            state.hwnd_edit,
                            state.hinstance,
                            status_h,
                        ) {
                            state.hwnd_edit = new_edit;
                        }
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
