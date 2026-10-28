//! Faithful, safe reverse-engineered reimplementation of Notepad's Menu Bar and `InitMenu` (`0x14000F2A0`).
//!
//! # Reverse Engineering Notes
//!
//! ## 1. Menu Command IDs (`NPCommand` at `0x14000DA7C` - `0x14000E68A`)
//! - **File Menu (Submenu 0):**
//!   - `IDM_FILENEW`    = `0x01` (1)  -> `0x14000DB05` -> `0x14000DF37` (`New(TRUE)` at `0x1400118A0`)
//!   - `IDM_FILEOPEN`   = `0x02` (2)  -> `0x14000DB0E` -> `0x14000DE00` (`GetOpenFileNameW` + `LoadFile` at `0x140010F48`)
//!   - `IDM_FILESAVE`   = `0x03` (3)  -> `0x14000DB17` -> `0x14000DC73` (`SaveFile` at `0x140011AF8`)
//!   - `IDM_FILESAVEAS` = `0x04` (4)  -> `0x14000DB20` -> `0x14000DCAF` (`GetSaveFileNameW` + `SaveFile`)
//!   - `IDM_FILEEXIT`   = `0x07` (7)  -> `0x14000DAFA` -> `0x14000DF4C` (posts `WM_CLOSE`)
//! - **Edit Menu (Submenu 1):**
//!   - `ID_EDIT_UNDO`       = `0x10` (16)
//!   - `ID_EDIT_CUT`        = `0x300` (`WM_CUT`)
//!   - `ID_EDIT_COPY`       = `0x301` (`WM_COPY`)
//!   - `ID_EDIT_PASTE`      = `0x302` (`WM_PASTE`)
//!   - `ID_EDIT_CLEAR`      = `0x303` (`WM_CLEAR`)
//!   - `ID_EDIT_FIND`       = `0x15` (21)
//!   - `ID_EDIT_FINDNEXT`   = `0x16` (22)
//!   - `ID_EDIT_GOTO`       = `0x18` (24)
//!   - `ID_EDIT_SELECTALL`  = `0x19` (25) -> `0x14000E3A0` (`EM_SETSEL(0, -1)` + `EM_SCROLLCARET`)
//!   - `ID_EDIT_TIMEDATE`   = `0x1A` (26) -> `0x14000E394` (`InsertDateTime` at `0x140010580`)
//!   - `ID_EDIT_BINGSEARCH` = `0x1C` (28)
//!   - `ID_EDIT_FINDPREV`   = `0x1D` (29)
//! - **Format Menu (Submenu 2):**
//!   - `ID_FORMAT_WORDWRAP` = `0x20` (32) -> `0x14000E1CE` (`NPReCreate` at `0x14001FA28`)
//! - **View Menu (Submenu 3):**
//!   - `ID_VIEW_STATUS`     = `0x1B` (27) -> `0x14000E30A` (`ShowWindow` + `NPSize`)
//! - **Help Menu (Submenu 4):**
//!   - `IDM_HELP_ABOUT`     = `0x41` (65) -> `0x14000E59C` (`ShellAboutW`)

use crate::theme::ThemePreset;
use crate::word_wrap::{WordWrap, FORMAT_MENU_INDEX, ID_FORMAT_WORDWRAP};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::DataExchange::{
    CloseClipboard, IsClipboardFormatAvailable, OpenClipboard,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuItem, CreateMenu, CreatePopupMenu, EnableMenuItem, GetMenu, GetSubMenu,
    SendMessageW, HMENU, MENU_ITEM_FLAGS, MF_BYCOMMAND, MF_CHECKED, MF_ENABLED, MF_GRAYED,
    MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, WM_GETTEXTLENGTH,
};

// Win32 Edit Control Messages
pub const EM_GETSEL: u32 = 0x00B0;
pub const EM_SETSEL: u32 = 0x00B1;
pub const EM_SCROLLCARET: u32 = 0x00B7;
pub const EM_CANUNDO: u32 = 0x00C6;
pub const EM_UNDO: u32 = 0x00C7;
pub const EM_REPLACESEL: u32 = 0x00C2;

// Clipboard formats
pub const CF_UNICODETEXT: u32 = 13;

// Notepad Menu Submenu Indices
pub const FILE_MENU_INDEX: i32 = 0;
pub const EDIT_MENU_INDEX: i32 = 1;
pub const VIEW_MENU_INDEX: i32 = 3;
pub const PREFERENCES_MENU_INDEX: i32 = 4;
pub const HELP_MENU_INDEX: i32 = 5;

// Notepad File Menu Command IDs (0x14000DAFA - 0x14000DB2E)
pub const IDM_FILENEW: u32 = 0x01;
pub const IDM_FILEOPEN: u32 = 0x02;
pub const IDM_FILESAVE: u32 = 0x03;
pub const IDM_FILESAVEAS: u32 = 0x04;
pub const IDM_FILEEXIT: u32 = 0x07;

// Notepad Edit Menu Command IDs
pub const ID_EDIT_UNDO: u32 = 0x10;
pub const ID_EDIT_CUT: u32 = 0x300;
pub const ID_EDIT_COPY: u32 = 0x301;
pub const ID_EDIT_PASTE: u32 = 0x302;
pub const ID_EDIT_CLEAR: u32 = 0x303;
pub const ID_EDIT_FIND: u32 = 0x15;
pub const ID_EDIT_FINDNEXT: u32 = 0x16;
pub const ID_EDIT_FINDPREV: u32 = 0x1D;
pub const ID_EDIT_GOTO: u32 = 0x18;
pub const ID_EDIT_SELECTALL: u32 = 0x19;
pub const ID_EDIT_TIMEDATE: u32 = 0x1A;
pub const ID_EDIT_BINGSEARCH: u32 = 0x1C;

// Multi-Cursor & Block Selection Command IDs
pub const ID_EDIT_ADD_CURSOR_ABOVE: u32 = 0x50;
pub const ID_EDIT_ADD_CURSOR_BELOW: u32 = 0x51;
pub const ID_EDIT_ADD_CURSOR_NEXT_MATCH: u32 = 0x52;
pub const ID_EDIT_ADD_CURSORS_LINE_ENDS: u32 = 0x53;
pub const ID_EDIT_SINGLE_CURSOR: u32 = 0x54;

// Notepad View & Help Command IDs
pub const ID_VIEW_STATUS: u32 = 0x1B;
pub const ID_VIEW_LINE_NUMBERS: u32 = 0x60;
pub const IDM_HELP_ABOUT: u32 = 0x41;

// Sublime-inspired Color Scheme Theme Command IDs (Format & Preferences Menus)
pub const ID_FORMAT_THEME_MARIANA: u32 = 0x61;
pub const ID_FORMAT_THEME_MONOKAI: u32 = 0x62;
pub const ID_FORMAT_THEME_BREAKERS: u32 = 0x63;
pub const ID_FORMAT_THEME_CLASSIC: u32 = 0x64;

// Sublime-inspired Preferences > Font > Larger / Smaller / Reset Size Command IDs
// (`Main.sublime-menu` lines 956-966: `increase_font_size`, `decrease_font_size`, `reset_font_size`, `choose_font`)
pub const ID_PREF_FONT_LARGER: u32 = 0x70;
pub const ID_PREF_FONT_SMALLER: u32 = 0x71;
pub const ID_PREF_FONT_RESET: u32 = 0x72;
pub const ID_PREF_FONT_CONSOLAS: u32 = 0x73;
pub const ID_PREF_FONT_CASCADIA: u32 = 0x74;
pub const ID_PREF_FONT_COURIER: u32 = 0x75;
pub const ID_PREF_SHOW_FULL_PATH: u32 = 0x76;

/// Represents the synchronized editor state inspected during `WM_INITMENU`.
#[derive(Debug, Clone, Copy)]
pub struct EditorState {
    pub word_wrap: WordWrap,
    pub status_bar_visible: bool,
    pub line_numbers_visible: bool,
    pub theme_preset: ThemePreset,
    pub show_full_path: bool,
}

fn append_menu_str(hmenu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = AppendMenuW(hmenu, flags, id, PCWSTR(wide.as_ptr()));
    }
}

/// Programmatically builds the complete 5-submenu Notepad menu bar matching
/// the exact submenu indices (`0..=4`) and command IDs expected by `InitMenu` and `NPCommand`.
pub fn create_notepad_menu() -> Result<HMENU, &'static str> {
    unsafe {
        let menu_bar = CreateMenu().map_err(|_| "CreateMenu failed")?;

        // Submenu 0: File
        let file_menu = CreatePopupMenu().map_err(|_| "CreatePopupMenu (File) failed")?;
        append_menu_str(file_menu, MF_STRING, IDM_FILENEW as usize, "&New\tCtrl+N");
        append_menu_str(file_menu, MF_STRING, IDM_FILEOPEN as usize, "&Open...\tCtrl+O");
        append_menu_str(file_menu, MF_STRING, IDM_FILESAVE as usize, "&Save\tCtrl+S");
        append_menu_str(
            file_menu,
            MF_STRING,
            IDM_FILESAVEAS as usize,
            "Save &As...\tCtrl+Shift+S",
        );
        let _ = AppendMenuW(file_menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_str(file_menu, MF_STRING, IDM_FILEEXIT as usize, "E&xit");
        append_menu_str(menu_bar, MF_POPUP, file_menu.0 as usize, "&File");

        // Submenu 1: Edit
        let edit_menu = CreatePopupMenu().map_err(|_| "CreatePopupMenu (Edit) failed")?;
        append_menu_str(edit_menu, MF_STRING, ID_EDIT_UNDO as usize, "&Undo\tCtrl+Z");
        let _ = AppendMenuW(edit_menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_str(edit_menu, MF_STRING, ID_EDIT_CUT as usize, "Cu&t\tCtrl+X");
        append_menu_str(edit_menu, MF_STRING, ID_EDIT_COPY as usize, "&Copy\tCtrl+C");
        append_menu_str(edit_menu, MF_STRING, ID_EDIT_PASTE as usize, "&Paste\tCtrl+V");
        append_menu_str(edit_menu, MF_STRING, ID_EDIT_CLEAR as usize, "De&lete\tDel");
        let _ = AppendMenuW(edit_menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_str(
            edit_menu,
            MF_STRING,
            ID_EDIT_BINGSEARCH as usize,
            "&Search with Bing...\tCtrl+E",
        );
        append_menu_str(edit_menu, MF_STRING, ID_EDIT_FIND as usize, "&Find...\tCtrl+F");
        append_menu_str(
            edit_menu,
            MF_STRING,
            ID_EDIT_FINDNEXT as usize,
            "Find &Next\tF3",
        );
        append_menu_str(
            edit_menu,
            MF_STRING,
            ID_EDIT_FINDPREV as usize,
            "Find &Previous\tShift+F3",
        );
        append_menu_str(edit_menu, MF_STRING, ID_EDIT_GOTO as usize, "&Go To...\tCtrl+G");
        let _ = AppendMenuW(edit_menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_str(
            edit_menu,
            MF_STRING,
            ID_EDIT_SELECTALL as usize,
            "Select &All\tCtrl+A",
        );
        append_menu_str(
            edit_menu,
            MF_STRING,
            ID_EDIT_TIMEDATE as usize,
            "Time/&Date\tF5",
        );
        let _ = AppendMenuW(edit_menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_str(
            edit_menu,
            MF_STRING,
            ID_EDIT_ADD_CURSOR_ABOVE as usize,
            "Add Cursor &Above\tCtrl+Alt+Up",
        );
        append_menu_str(
            edit_menu,
            MF_STRING,
            ID_EDIT_ADD_CURSOR_BELOW as usize,
            "Add Cursor &Below\tCtrl+Alt+Down",
        );
        append_menu_str(
            edit_menu,
            MF_STRING,
            ID_EDIT_ADD_CURSOR_NEXT_MATCH as usize,
            "Add Cursor at Next &Match\tCtrl+D",
        );
        append_menu_str(
            edit_menu,
            MF_STRING,
            ID_EDIT_ADD_CURSORS_LINE_ENDS as usize,
            "Add Cursors to Line &Ends\tAlt+Shift+I",
        );
        append_menu_str(
            edit_menu,
            MF_STRING,
            ID_EDIT_SINGLE_CURSOR as usize,
            "Collapse to &Single Cursor\tEsc",
        );
        append_menu_str(menu_bar, MF_POPUP, edit_menu.0 as usize, "&Edit");

        // Submenu 2: Format
        let format_menu = CreatePopupMenu().map_err(|_| "CreatePopupMenu (Format) failed")?;
        append_menu_str(
            format_menu,
            MF_STRING,
            ID_FORMAT_WORDWRAP as usize,
            "&Word Wrap",
        );
        let _ = AppendMenuW(format_menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_str(
            format_menu,
            MF_STRING,
            ID_FORMAT_THEME_MARIANA as usize,
            "Theme: &Mariana (Sublime Dark)",
        );
        append_menu_str(
            format_menu,
            MF_STRING,
            ID_FORMAT_THEME_MONOKAI as usize,
            "Theme: M&onokai (Sublime Dark)",
        );
        append_menu_str(
            format_menu,
            MF_STRING,
            ID_FORMAT_THEME_BREAKERS as usize,
            "Theme: &Breakers (Sublime Light)",
        );
        append_menu_str(
            format_menu,
            MF_STRING,
            ID_FORMAT_THEME_CLASSIC as usize,
            "Theme: &Classic Notepad (Light)",
        );
        append_menu_str(menu_bar, MF_POPUP, format_menu.0 as usize, "F&ormat");

        // Submenu 3: View
        let view_menu = CreatePopupMenu().map_err(|_| "CreatePopupMenu (View) failed")?;
        append_menu_str(view_menu, MF_STRING, ID_VIEW_STATUS as usize, "&Status Bar");
        append_menu_str(
            view_menu,
            MF_STRING,
            ID_VIEW_LINE_NUMBERS as usize,
            "&Line Numbers (Gutter)",
        );
        append_menu_str(menu_bar, MF_POPUP, view_menu.0 as usize, "&View");

        // Submenu 4: Preferences (Sublime Text `Main.sublime-menu` lines 900-968)
        let pref_menu = CreatePopupMenu().map_err(|_| "CreatePopupMenu (Preferences) failed")?;
        let font_submenu = CreatePopupMenu().map_err(|_| "CreatePopupMenu (Font) failed")?;
        append_menu_str(
            font_submenu,
            MF_STRING,
            ID_PREF_FONT_LARGER as usize,
            "&Larger\tCtrl+Plus",
        );
        append_menu_str(
            font_submenu,
            MF_STRING,
            ID_PREF_FONT_SMALLER as usize,
            "&Smaller\tCtrl+Minus",
        );
        let _ = AppendMenuW(font_submenu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_str(
            font_submenu,
            MF_STRING,
            ID_PREF_FONT_RESET as usize,
            "&Reset Size\tCtrl+0",
        );
        let _ = AppendMenuW(font_submenu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_str(
            font_submenu,
            MF_STRING,
            ID_PREF_FONT_CONSOLAS as usize,
            "Choose: &Consolas (Default)",
        );
        append_menu_str(
            font_submenu,
            MF_STRING,
            ID_PREF_FONT_CASCADIA as usize,
            "Choose: C&ascadia Mono",
        );
        append_menu_str(
            font_submenu,
            MF_STRING,
            ID_PREF_FONT_COURIER as usize,
            "Choose: Courier &New",
        );
        append_menu_str(pref_menu, MF_POPUP, font_submenu.0 as usize, "&Font");
        let _ = AppendMenuW(pref_menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_str(
            pref_menu,
            MF_STRING,
            ID_PREF_SHOW_FULL_PATH as usize,
            "Show &Full Path in Title Bar",
        );
        append_menu_str(menu_bar, MF_POPUP, pref_menu.0 as usize, "Prefere&nces");

        // Submenu 5: Help
        let help_menu = CreatePopupMenu().map_err(|_| "CreatePopupMenu (Help) failed")?;
        append_menu_str(
            help_menu,
            MF_STRING,
            IDM_HELP_ABOUT as usize,
            "&About Notepad",
        );
        append_menu_str(menu_bar, MF_POPUP, help_menu.0 as usize, "&Help");

        Ok(menu_bar)
    }
}

/// Safely evaluates and synchronizes the entire menu state for Notepad (`InitMenu` at `0x14000F2A0`).
pub fn init_menu(
    hwnd_main: HWND,
    hwnd_edit: HWND,
    state: &EditorState,
) -> Result<(), &'static str> {
    if hwnd_main.is_invalid() || hwnd_edit.is_invalid() {
        return Err("Invalid HWND provided");
    }

    unsafe {
        let hmenu: HMENU = GetMenu(hwnd_main);
        if hmenu.is_invalid() {
            return Err("GetMenu returned null handle");
        }

        let edit_menu = GetSubMenu(hmenu, EDIT_MENU_INDEX);
        let format_menu = GetSubMenu(hmenu, FORMAT_MENU_INDEX);
        let view_menu = GetSubMenu(hmenu, VIEW_MENU_INDEX);
        let pref_menu = GetSubMenu(hmenu, PREFERENCES_MENU_INDEX);

        // 1. Check selection state (SendMessageW EM_GETSEL at 0x14000F2EC)
        let mut sel_start: u32 = 0;
        let mut sel_end: u32 = 0;
        SendMessageW(
            hwnd_edit,
            EM_GETSEL,
            Some(WPARAM(&mut sel_start as *mut u32 as usize)),
            Some(LPARAM(&mut sel_end as *mut u32 as isize)),
        );

        let has_selection = sel_start != sel_end;
        let sel_flag = if has_selection { MF_ENABLED } else { MF_GRAYED } | MF_BYCOMMAND;

        if !edit_menu.is_invalid() {
            let _ = EnableMenuItem(edit_menu, ID_EDIT_CUT, sel_flag);
            let _ = EnableMenuItem(edit_menu, ID_EDIT_COPY, sel_flag);
            let _ = EnableMenuItem(edit_menu, ID_EDIT_CLEAR, sel_flag);
            let _ = EnableMenuItem(edit_menu, ID_EDIT_BINGSEARCH, sel_flag);
        }

        // 2. Check Clipboard format availability (0x14000F3AE - 0x14000F3D0)
        let mut has_clipboard_text = false;
        if OpenClipboard(Some(hwnd_main)).is_ok() {
            has_clipboard_text = IsClipboardFormatAvailable(CF_UNICODETEXT).is_ok();
            let _ = CloseClipboard();
        }

        if !edit_menu.is_invalid() {
            let paste_flag = if has_clipboard_text {
                MF_ENABLED
            } else {
                MF_GRAYED
            } | MF_BYCOMMAND;
            let _ = EnableMenuItem(edit_menu, ID_EDIT_PASTE, paste_flag);
        }

        // 3. Check Text Length (WM_GETTEXTLENGTH at 0x14000F41E)
        let text_len = SendMessageW(hwnd_edit, WM_GETTEXTLENGTH, None, None).0;
        let has_text = text_len > 0;
        let search_flag = if has_text { MF_ENABLED } else { MF_GRAYED } | MF_BYCOMMAND;

        if !edit_menu.is_invalid() {
            let _ = EnableMenuItem(edit_menu, ID_EDIT_FIND, search_flag);
            let _ = EnableMenuItem(edit_menu, ID_EDIT_FINDNEXT, search_flag);
            let _ = EnableMenuItem(edit_menu, ID_EDIT_FINDPREV, search_flag);

            // 4. Word wrap restricts "Go To..." (ID_EDIT_GOTO at 0x14000F4AD - 0x14000F4D5)
            let goto_flag = if state.word_wrap.is_enabled() {
                MF_GRAYED
            } else {
                MF_ENABLED
            } | MF_BYCOMMAND;
            let _ = EnableMenuItem(edit_menu, ID_EDIT_GOTO, goto_flag);

            // 5. Check Undo Capability (EM_CANUNDO at 0x14000F4F3)
            let can_undo = SendMessageW(hwnd_edit, EM_CANUNDO, None, None).0 != 0;
            let undo_flag = if can_undo { MF_ENABLED } else { MF_GRAYED } | MF_BYCOMMAND;
            let _ = EnableMenuItem(edit_menu, ID_EDIT_UNDO, undo_flag);
        }

        // 6. Format Menu: CheckMenuItem for Word Wrap (0x14000F55A) & Active Theme Preset
        if !format_menu.is_invalid() {
            let wrap_check: MENU_ITEM_FLAGS = state.word_wrap.menu_check_flag() | MF_BYCOMMAND;
            CheckMenuItem(format_menu, ID_FORMAT_WORDWRAP, wrap_check.0);

            for (cmd_id, preset) in [
                (ID_FORMAT_THEME_MARIANA, ThemePreset::MarianaDark),
                (ID_FORMAT_THEME_MONOKAI, ThemePreset::MonokaiDark),
                (ID_FORMAT_THEME_BREAKERS, ThemePreset::BreakersLight),
                (ID_FORMAT_THEME_CLASSIC, ThemePreset::ClassicNotepad),
            ] {
                let flag = if state.theme_preset == preset {
                    MF_CHECKED
                } else {
                    MF_UNCHECKED
                } | MF_BYCOMMAND;
                CheckMenuItem(format_menu, cmd_id, flag.0);
            }
        }

        // 7. View Menu: CheckMenuItem for Status Bar (0x14000F591) & Line Numbers Gutter
        if !view_menu.is_invalid() {
            let status_check = if state.status_bar_visible {
                MF_CHECKED
            } else {
                MF_UNCHECKED
            } | MF_BYCOMMAND;
            CheckMenuItem(view_menu, ID_VIEW_STATUS, status_check.0);

            let gutter_check = if state.line_numbers_visible {
                MF_CHECKED
            } else {
                MF_UNCHECKED
            } | MF_BYCOMMAND;
            CheckMenuItem(view_menu, ID_VIEW_LINE_NUMBERS, gutter_check.0);
        }

        // 8. Preferences Menu: CheckMenuItem for Show Full Path in Title Bar
        if !pref_menu.is_invalid() {
            let full_path_check = if state.show_full_path {
                MF_CHECKED
            } else {
                MF_UNCHECKED
            } | MF_BYCOMMAND;
            CheckMenuItem(pref_menu, ID_PREF_SHOW_FULL_PATH, full_path_check.0);
        }
    }

    Ok(())
}
