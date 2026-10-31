use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Controls::SetWindowTheme;

pub fn apply_dark_scrollbar(hwnd: HWND) {
    let dark_mode_str: Vec<u16> = "DarkMode_Explorer\0".encode_utf16().collect();
    unsafe {
        let _ = SetWindowTheme(hwnd, PCWSTR(dark_mode_str.as_ptr()), PCWSTR::null());
    }
}
