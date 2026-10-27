//! Reverse-engineered Word Wrap state management, Edit control recreation (`NPReCreate`),
//! and Win32 UI synchronization (`CheckMenuItem`).
//!
//! # Reverse Engineering Notes
//!
//! In classic Notepad (`C:\Windows\System32\notepad.exe`):
//! - **Menu Check Address:** `0x14000F55A` (RVA `+0xF55A`), called within `InitMenu` (`0x14000F2A0`).
//! - **Menu Structure:** Submenu 2 corresponds to the "Format" menu.
//! - **Command ID:** `ID_FORMAT_WORDWRAP` = `0x20` (decimal 32).
//! - **Global State:** Stored at RVA `+0x3642C` (virtual address `0x14003642C`).
//! - **Edit Recreation (`NPReCreate` at `0x14001FA28`, called from `0x14000E1E4`):**
//!   - In `NPInit` (`0x14001366A`), the base unwrapped style is `0x50300104`:
//!     `WS_CHILD (0x40000000) | WS_VISIBLE (0x10000000) | WS_VSCROLL (0x00200000) | WS_HSCROLL (0x00100000) | ES_MULTILINE (0x0004) | ES_NOHIDESEL (0x0100)`.
//!   - When Word Wrap is enabled, `0x00100000` (`WS_HSCROLL`) is stripped, yielding `0x50200104`.
//!   - Because Win32 `Edit` controls cannot dynamically toggle `WS_HSCROLL` after creation,
//!     `NPReCreate` preserves the existing text, selection (`EM_GETSEL`), font (`WM_GETFONT`),
//!     and modified state (`EM_GETMODIFY`), destroys the old control, and calls `CreateWindowExW`
//!     (`0x1400125A0` / `0x1400125E8`) with control ID `0x0F` (15).

use crate::file_io::{get_edit_text, is_edit_modified, set_edit_modified, set_edit_text};
use crate::menu::EM_GETSEL;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    CheckMenuItem, CreateWindowExW, DestroyWindow, GetClientRect, GetSubMenu, SendMessageW,
    HMENU, MENU_ITEM_FLAGS, MF_BYCOMMAND, MF_CHECKED, MF_UNCHECKED, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_GETFONT, WM_SETFONT,
};

/// Command ID for Format -> Word Wrap in classic Notepad (`0x20` = 32).
pub const ID_FORMAT_WORDWRAP: u32 = 0x20;

/// Child control ID for the `"Edit"` window (`0x0F` at `0x1400125BC`).
pub const ID_EDIT_CONTROL: usize = 0x0F;

/// Submenu index for "Format" (File=0, Edit=1, Format=2, View=3, Help=4).
pub const FORMAT_MENU_INDEX: i32 = 2;

/// Base multiline edit control styles used by Notepad (`0x50200104` at `0x14000E1DE`):
/// `WS_CHILD (0x40000000) | WS_VISIBLE (0x10000000) | WS_VSCROLL (0x00200000) |
///  ES_MULTILINE (0x0004) | ES_NOHIDESEL (0x0100)`
pub const NOTEPAD_BASE_EDIT_STYLE: u32 = 0x5020_0104;

/// Horizontal scroll style: `WS_HSCROLL (0x00100000)` (`0x14000E1D8`).
pub const WS_HSCROLL_STYLE: u32 = 0x0010_0000;

/// Edit control message to set selection range (`0x00B1` at `0x14000E3AE`).
pub const EM_SETSEL: u32 = 0x00B1;

/// Represents the Word Wrap state of the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordWrap {
    Disabled,
    Enabled,
}

impl WordWrap {
    /// Returns true if word wrap is enabled.
    #[inline]
    pub fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }

    /// Toggles the word wrap state.
    #[inline]
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Disabled => Self::Enabled,
            Self::Enabled => Self::Disabled,
        }
    }

    /// Computes the corresponding Win32 `MENU_ITEM_FLAGS` check flag.
    ///
    /// Mirrors the x86-64 bitwise logic (`0x14000F539` - `0x14000F540`):
    /// `neg eax; sbb ebx, ebx; and ebx, 8;`
    #[inline]
    pub fn menu_check_flag(self) -> MENU_ITEM_FLAGS {
        if self.is_enabled() {
            MF_CHECKED
        } else {
            MF_UNCHECKED
        }
    }

    /// Computes the Win32 window style for the `"Edit"` control (`0x14000E1CE` - `0x14000E1DE`).
    #[inline]
    pub fn edit_window_style(self) -> u32 {
        if self.is_enabled() {
            NOTEPAD_BASE_EDIT_STYLE
        } else {
            NOTEPAD_BASE_EDIT_STYLE | WS_HSCROLL_STYLE
        }
    }
}

/// Safe manager for Notepad's Word Wrap state and Win32 menu synchronization.
#[derive(Debug)]
pub struct WordWrapManager {
    state: WordWrap,
}

impl WordWrapManager {
    /// Creates a new `WordWrapManager` initialized to the given state.
    pub fn new(initial_state: WordWrap) -> Self {
        Self {
            state: initial_state,
        }
    }

    /// Returns the current word wrap state.
    pub fn state(&self) -> WordWrap {
        self.state
    }

    /// Toggles word wrap state and returns the new value.
    pub fn toggle(&mut self) -> WordWrap {
        self.state = self.state.toggle();
        self.state
    }

    /// Updates the checkmark state of the "Word Wrap" item in the top-level menu bar (`0x14000F55A`).
    pub fn sync_menu_item(&self, top_menu: HMENU) -> Result<(), &'static str> {
        if top_menu.is_invalid() {
            return Err("Invalid top-level HMENU handle");
        }

        unsafe {
            let format_menu = GetSubMenu(top_menu, FORMAT_MENU_INDEX);
            if format_menu.is_invalid() {
                return Err("Failed to retrieve Format submenu");
            }

            let check_flag = self.state.menu_check_flag() | MF_BYCOMMAND;
            let ret = CheckMenuItem(format_menu, ID_FORMAT_WORDWRAP, check_flag.0);
            if ret == u32::MAX {
                return Err("CheckMenuItem failed to find or modify menu item");
            }
        }

        Ok(())
    }

    /// Toggles Word Wrap and recreates the child `"Edit"` control (`NPReCreate` at `0x14001FA28`),
    /// preserving text content, cursor selection, font handle, and dirty flag.
    pub fn toggle_and_recreate_edit(
        &mut self,
        hwnd_main: HWND,
        hwnd_edit: HWND,
        hinstance: HINSTANCE,
        status_bar_height: i32,
    ) -> Result<HWND, &'static str> {
        let next_state = self.state.toggle();
        let new_edit = recreate_edit_control(
            hwnd_main,
            hwnd_edit,
            hinstance,
            next_state,
            status_bar_height,
        )?;
        self.state = next_state;
        Ok(new_edit)
    }
}

/// Spawns the child `"Edit"` control matching helper `0x1400125A0` (`CreateWindowExW` at `0x1400125E8`).
pub fn create_edit_control(
    hwnd_main: HWND,
    hinstance: HINSTANCE,
    width: i32,
    height: i32,
    wrap: WordWrap,
) -> Result<HWND, &'static str> {
    let class_edit: Vec<u16> = "Edit\0".encode_utf16().collect();
    let empty_title: [u16; 1] = [0];
    let style = WINDOW_STYLE(wrap.edit_window_style());

    let hwnd_edit = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_edit.as_ptr()),
            PCWSTR(empty_title.as_ptr()),
            style,
            0,
            0,
            width,
            height,
            Some(hwnd_main),
            Some(HMENU(ID_EDIT_CONTROL as *mut core::ffi::c_void)),
            Some(hinstance),
            None,
        )
        .map_err(|_| "CreateWindowExW for Edit control failed")?
    };

    Ok(hwnd_edit)
}

/// Recreates the child `"Edit"` control with updated styles (`NPReCreate` at `0x14001FA28`).
pub fn recreate_edit_control(
    hwnd_main: HWND,
    old_edit: HWND,
    hinstance: HINSTANCE,
    new_wrap: WordWrap,
    status_bar_height: i32,
) -> Result<HWND, &'static str> {
    if hwnd_main.is_invalid() || old_edit.is_invalid() {
        return Err("Invalid window handle passed to recreate_edit_control");
    }

    // 1. Preserve current state (text, selection, font, modified flag)
    let saved_text = get_edit_text(old_edit);
    let was_modified = is_edit_modified(old_edit);

    let mut sel_start: u32 = 0;
    let mut sel_end: u32 = 0;
    let hfont = unsafe {
        SendMessageW(
            old_edit,
            EM_GETSEL,
            Some(WPARAM(&mut sel_start as *mut u32 as usize)),
            Some(LPARAM(&mut sel_end as *mut u32 as isize)),
        );
        SendMessageW(old_edit, WM_GETFONT, None, None)
    };

    // 2. Measure current client area of main window
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd_main, &mut rc);
    }
    let width = (rc.right - rc.left).max(0);
    let height = (rc.bottom - rc.top - status_bar_height).max(0);

    // 3. Create replacement Edit control with the new WordWrap style
    let new_edit = create_edit_control(hwnd_main, hinstance, width, height, new_wrap)?;

    // 4. Destroy the old Edit control and restore state onto the new handle
    unsafe {
        let _ = DestroyWindow(old_edit);
        if hfont.0 != 0 {
            SendMessageW(
                new_edit,
                WM_SETFONT,
                Some(WPARAM(hfont.0 as usize)),
                Some(LPARAM(1)),
            );
        }
    }

    set_edit_text(new_edit, &saved_text);
    set_edit_modified(new_edit, was_modified);

    unsafe {
        SendMessageW(
            new_edit,
            EM_SETSEL,
            Some(WPARAM(sel_start as usize)),
            Some(LPARAM(sel_end as isize)),
        );
        let _ = SetFocus(Some(new_edit));
    }

    Ok(new_edit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_wrap_toggle() {
        let mut manager = WordWrapManager::new(WordWrap::Disabled);
        assert_eq!(manager.state(), WordWrap::Disabled);
        assert_eq!(manager.state().menu_check_flag(), MF_UNCHECKED);
        assert_eq!(
            manager.state().edit_window_style(),
            NOTEPAD_BASE_EDIT_STYLE | WS_HSCROLL_STYLE
        );

        let new_state = manager.toggle();
        assert_eq!(new_state, WordWrap::Enabled);
        assert_eq!(manager.state().menu_check_flag(), MF_CHECKED);
        assert_eq!(
            manager.state().edit_window_style(),
            NOTEPAD_BASE_EDIT_STYLE
        );
    }
}
