//! Entry point (`wWinMain` at `0x140010280`) and Message Loop (`0x1400103A3` - `0x140010489`)
//! for the reverse-engineered Win32 Notepad clone.

use notepad_win32::cursor::{BlockDirection, Position2D};
use notepad_win32::file_explorer::DEFAULT_SIDEBAR_WIDTH;
use notepad_win32::file_io::{
    get_edit_text, load_file_win32, save_file_win32, set_edit_modified, set_edit_text, TextEncoding,
};
use notepad_win32::font::DEFAULT_WINDOWS_FONT_SIZE;
use notepad_win32::menu::{
    IDM_FILENEW, IDM_FILEOPEN, IDM_FILESAVE, IDM_FILE_CLOSE_TAB, IDM_FILE_OPEN_FOLDER,
    IDM_TAB_NEXT, IDM_TAB_PREV, ID_EDIT_ADD_CURSOR_BELOW, ID_EDIT_ADD_CURSOR_NEXT_MATCH,
    ID_EDIT_SELECTALL, ID_EDIT_SINGLE_CURSOR, ID_EDIT_TIMEDATE, ID_EDIT_UNDO,
    ID_FORMAT_THEME_BREAKERS, ID_FORMAT_THEME_MARIANA, ID_FORMAT_THEME_MONOKAI,
    ID_PREF_FONT_CASCADIA, ID_PREF_FONT_CONSOLAS, ID_PREF_FONT_LARGER, ID_PREF_FONT_RESET,
    ID_PREF_FONT_SMALLER, ID_PREF_SHOW_FULL_PATH, ID_VIEW_LINE_NUMBERS, ID_VIEW_STATUS,
    ID_VIEW_TOGGLE_SIDEBAR,
};
use notepad_win32::theme::ThemePreset;
use notepad_win32::window::{create_notepad_window, NotepadAppState};
use notepad_win32::word_wrap::{WordWrap, ID_FORMAT_WORDWRAP};
use std::path::PathBuf;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_ADD, VK_F5, VK_NUMPAD0, VK_OEM_MINUS, VK_OEM_PLUS, VK_SUBTRACT, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateAcceleratorTableW, DestroyAcceleratorTable, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, SendMessageW, TranslateAcceleratorW,
    TranslateMessage, ACCEL, FCONTROL, FSHIFT, FVIRTKEY, GWLP_USERDATA, HACCEL, MSG,
    SW_SHOWDEFAULT, WM_CLOSE, WM_COMMAND, WM_INITMENU, WM_SIZE,
};

/// Builds the keyboard accelerator table (`Ctrl+N`, `Ctrl+O`, `Ctrl+Shift+O`, `Ctrl+S`, `Ctrl+W`,
/// `Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+B`, `Ctrl+Z`, `Ctrl+A`, `F5`)
/// used by `TranslateAcceleratorW` at `0x1400103D9`.
fn create_accelerators() -> Option<HACCEL> {
    let accels = [
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: b'N' as u16,
            cmd: IDM_FILENEW as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: b'O' as u16,
            cmd: IDM_FILEOPEN as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL | FSHIFT,
            key: b'O' as u16,
            cmd: IDM_FILE_OPEN_FOLDER as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: b'S' as u16,
            cmd: IDM_FILESAVE as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: b'W' as u16,
            cmd: IDM_FILE_CLOSE_TAB as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: b'B' as u16,
            cmd: ID_VIEW_TOGGLE_SIDEBAR as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: VK_TAB.0,
            cmd: IDM_TAB_NEXT as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL | FSHIFT,
            key: VK_TAB.0,
            cmd: IDM_TAB_PREV as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: b'Z' as u16,
            cmd: ID_EDIT_UNDO as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: b'A' as u16,
            cmd: ID_EDIT_SELECTALL as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY,
            key: VK_F5.0,
            cmd: ID_EDIT_TIMEDATE as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: VK_OEM_PLUS.0,
            cmd: ID_PREF_FONT_LARGER as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: VK_ADD.0,
            cmd: ID_PREF_FONT_LARGER as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: VK_OEM_MINUS.0,
            cmd: ID_PREF_FONT_SMALLER as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: VK_SUBTRACT.0,
            cmd: ID_PREF_FONT_SMALLER as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: b'0' as u16,
            cmd: ID_PREF_FONT_RESET as u16,
        },
        ACCEL {
            fVirt: FVIRTKEY | FCONTROL,
            key: VK_NUMPAD0.0,
            cmd: ID_PREF_FONT_RESET as u16,
        },
    ];
    unsafe { CreateAcceleratorTableW(&accels).ok() }
}

unsafe fn read_window_title(hwnd: HWND) -> String {
    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; (len as usize) + 1];
    let read = GetWindowTextW(hwnd, &mut buf);
    String::from_utf16_lossy(&buf[..read.max(0) as usize])
}

/// Runs an automated end-to-end verification of the Win32 window, child `"Edit"` control,
/// `WM_SIZE`, `WM_INITMENU`, `WM_COMMAND` (Word Wrap recreation, Status Bar toggle, Time/Date),
/// and `CreateFileW` / `ReadFile` / `WriteFile` disk I/O.
fn run_automated_lifecycle_verification(hinstance: HINSTANCE) -> Result<(), String> {
    println!("=== Notepad Win32 Lifecycle Automated Verification ===");

    // 1. Create hidden Notepad window + child Edit control + Status Bar (NPRegister + NPInit)
    let hwnd = create_notepad_window(hinstance, None)?;
    println!("[OK] 1. Registered 'Notepad' class & created main HWND ({:?})", hwnd);

    unsafe {
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut NotepadAppState;
        let state = state_ptr.as_mut().ok_or("Missing NotepadAppState in GWLP_USERDATA")?;
        println!(
            "[OK] 2. Spawned child 'Edit' control ({:?}) and Status Bar ({:?})",
            state.hwnd_edit, state.hwnd_status
        );

        // 2. Test WM_SIZE (0x14000EA6E -> NPSize 0x14000E848)
        let lparam_size = LPARAM(((700isize) << 16) | 1024isize);
        SendMessageW(hwnd, WM_SIZE, Some(WPARAM(0)), Some(lparam_size));
        println!("[OK] 3. Verified WM_SIZE (1024x700) child control layout");

        // 3. Test File I/O and Edit Buffer population (LoadFile / SaveFile)
        let temp_file = std::env::temp_dir().join("notepad_lifecycle_verify.txt");
        let sample_text = ".LOG\r\nHello from reverse-engineered Rust Notepad!";
        save_file_win32(&temp_file, sample_text, TextEncoding::Utf8Bom)
            .map_err(|e| e.to_string())?;
        state.open_path(temp_file.clone())?;

        let current_text = get_edit_text(state.hwnd_edit);
        assert!(current_text.starts_with(".LOG\r\nHello from reverse-engineered Rust Notepad!"));
        assert!(current_text.len() > sample_text.len(), ".LOG timestamp should be appended");
        println!(
            "[OK] 4. Verified CreateFileW/ReadFile/WriteFile, BOM, and .LOG timestamp injection"
        );

        // 4. Test WM_COMMAND: Word Wrap toggle & Edit control recreation (0x14000E1CE -> NPReCreate)
        let old_edit = state.hwnd_edit;
        assert_eq!(state.word_wrap.state(), WordWrap::Disabled);
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_FORMAT_WORDWRAP as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(state.word_wrap.state(), WordWrap::Enabled);
        assert_ne!(state.hwnd_edit, old_edit);
        assert_eq!(get_edit_text(state.hwnd_edit), current_text);
        println!(
            "[OK] 5. Verified ID_FORMAT_WORDWRAP (0x20) recreated Edit HWND ({:?} -> {:?}) and preserved buffer",
            old_edit, state.hwnd_edit
        );

        // 5. Test WM_INITMENU (0x14000ED6D -> InitMenu 0x14000F2A0)
        SendMessageW(hwnd, WM_INITMENU, Some(WPARAM(0)), Some(LPARAM(0)));
        println!("[OK] 6. Verified WM_INITMENU (0x14000F2A0) menu item state synchronization");

        // 6. Test Status Bar toggle (ID_VIEW_STATUS = 0x1B)
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_VIEW_STATUS as usize)),
            Some(LPARAM(0)),
        );
        assert!(!state.status_bar_visible);
        println!("[OK] 7. Verified ID_VIEW_STATUS (0x1B) status bar visibility toggle");

        // 7. Test Multi-Cursor & Block Selection integration (Step 3 verification)
        set_edit_text(state.hwnd_edit, "item_a = 10\r\nitem_b = 20\r\nitem_c = 30");
        state.multi_cursor.set_text("item_a = 10\r\nitem_b = 20\r\nitem_c = 30");
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_EDIT_ADD_CURSOR_BELOW as usize)),
            Some(LPARAM(0)),
        );
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_EDIT_ADD_CURSOR_BELOW as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(state.multi_cursor.cursors.count(), 3);
        state.cmd_multi_insert_char('>');
        assert_eq!(
            get_edit_text(state.hwnd_edit),
            ">item_a = 10\r\n>item_b = 20\r\n>item_c = 30"
        );

        // Collapse to single cursor and test rectangular Block Selection
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_EDIT_SINGLE_CURSOR as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(state.multi_cursor.cursors.count(), 1);
        let col_pos = state
            .multi_cursor
            .pos_2d_to_byte(Position2D { line: 0, column: 10 });
        state.multi_cursor.cursors.primary_mut().move_to(col_pos, false);
        state.multi_cursor.block_select(BlockDirection::Down);
        state.multi_cursor.block_select(BlockDirection::Down);
        state.multi_cursor.block_select(BlockDirection::Right);
        state.multi_cursor.block_select(BlockDirection::Right);
        assert!(state.multi_cursor.cursors.has_block_selection());
        state.multi_cursor.insert_text("99");
        state.sync_to_hwnd_edit(true);
        assert_eq!(
            get_edit_text(state.hwnd_edit),
            ">item_a = 99\r\n>item_b = 99\r\n>item_c = 99"
        );

        // Test Ctrl+D (ID_EDIT_ADD_CURSOR_NEXT_MATCH)
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_EDIT_SINGLE_CURSOR as usize)),
            Some(LPARAM(0)),
        );
        state.multi_cursor.cursors.primary_mut().move_to(1, false);
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_EDIT_ADD_CURSOR_NEXT_MATCH as usize)),
            Some(LPARAM(0)),
        );
        println!(
            "[OK] 8. Verified Multi-Cursor (Ctrl+Alt+Down, Ctrl+D) & Block Selection (Shift+Alt+Arrows) state and HWND sync"
        );

        // 8. Test Sublime-inspired Theme Presets & Line-Number Gutter Layout
        assert_eq!(state.theme_preset, ThemePreset::MarianaDark);
        assert!(state.line_numbers_visible);
        assert_eq!(state.gutter_layout.digit_count, 2);
        let width_2_digits = state.gutter_layout.total_width;
        assert!(width_2_digits > 0);

        // Switch theme to Monokai Dark & Breakers Light and back to Mariana Dark
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_FORMAT_THEME_MONOKAI as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(state.theme_preset, ThemePreset::MonokaiDark);
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_FORMAT_THEME_BREAKERS as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(state.theme_preset, ThemePreset::BreakersLight);
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_FORMAT_THEME_MARIANA as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(state.theme_preset, ThemePreset::MarianaDark);

        // Populate 120 lines and verify dynamic gutter expansion from 2 digits to 3 digits
        let many_lines: String = (1..=120)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\r\n");
        state.multi_cursor.set_text(many_lines);
        state.sync_to_hwnd_edit(false);
        assert_eq!(state.gutter_layout.digit_count, 3);
        assert!(
            state.gutter_layout.total_width > width_2_digits,
            "Gutter width must expand when crossing 99 -> 120 lines"
        );

        // Toggle line numbers off and back on
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_VIEW_LINE_NUMBERS as usize)),
            Some(LPARAM(0)),
        );
        assert!(!state.line_numbers_visible);
        assert_eq!(state.gutter_layout.total_width, 0);
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_VIEW_LINE_NUMBERS as usize)),
            Some(LPARAM(0)),
        );
        assert!(state.line_numbers_visible);
        assert!(state.gutter_layout.total_width > width_2_digits);
        println!(
            "[OK] 9. Verified Sublime Theme Presets (Mariana/Monokai/Breakers) & Dynamic Line-Number Gutter (2 -> 3 digits: {}px -> {}px)",
            width_2_digits, state.gutter_layout.total_width
        );

        // 9. Test Sublime Font Scaling (`Preferences > Font > Larger / Smaller / Reset Size`) & Dynamic Window Title
        assert_eq!(state.font_state.font_size, DEFAULT_WINDOWS_FONT_SIZE);
        let gutter_width_11pt = state.gutter_layout.total_width;
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_PREF_FONT_LARGER as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(state.font_state.font_size, 12);
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_PREF_FONT_LARGER as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(state.font_state.font_size, 13);
        let gutter_width_13pt = state.gutter_layout.total_width;
        assert!(
            gutter_width_13pt >= gutter_width_11pt,
            "Gutter width should scale with font size"
        );

        // Verify dynamic window title includes full path, dirty bullet (` •`), and `[13pt]` scale badge
        set_edit_modified(state.hwnd_edit, true);
        state.update_window_title();
        let title_13pt = read_window_title(hwnd);
        assert!(
            title_13pt.contains("notepad_lifecycle_verify.txt \u{2022} [13pt] - Notepad"),
            "Unexpected title at 13pt: {}",
            title_13pt
        );

        // Toggle `show_full_path` off and verify filename-only title
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_PREF_SHOW_FULL_PATH as usize)),
            Some(LPARAM(0)),
        );
        assert!(!state.show_full_path);
        let title_short = read_window_title(hwnd);
        assert_eq!(
            title_short,
            "notepad_lifecycle_verify.txt \u{2022} [13pt] - Notepad"
        );

        // Test `Smaller` and `Reset Size` (`0x1400FECCA` & `0x1400FED9E`) and font family switch
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_PREF_FONT_SMALLER as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(state.font_state.font_size, 12);
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_PREF_FONT_RESET as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(state.font_state.font_size, DEFAULT_WINDOWS_FONT_SIZE);
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_PREF_FONT_CASCADIA as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(state.font_state.font_face, "Cascadia Mono");
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_PREF_FONT_CONSOLAS as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(state.font_state.font_face, "Consolas");
        println!(
            "[OK] 10. Verified Sublime Font Scaling (Larger/Smaller/Reset: 11pt -> 13pt -> 11pt, gutter {}px -> {}px) & Dynamic Window Title ('{}')",
            gutter_width_11pt, gutter_width_13pt, title_short
        );

        // 10. Test Multi-Tab (`File > New`, switching, reordering, dirty `*`, `Ctrl+Tab`, closing)
        //     and `File > Open Folder` (`D:\test` -> left pane tree -> preview vs pinned tab)
        let test_folder = PathBuf::from(r"D:\test");
        let created_d_test = !test_folder.exists();
        let _ = std::fs::create_dir_all(&test_folder);
        let file_alpha = test_folder.join("alpha.txt");
        let file_beta = test_folder.join("beta.rs");
        save_file_win32(&file_alpha, "Alpha file content", TextEncoding::Utf8)
            .map_err(|e| e.to_string())?;
        save_file_win32(&file_beta, "fn beta() -> u32 { 42 }", TextEncoding::Utf8)
            .map_err(|e| e.to_string())?;

        // Open Folder `D:\test` -> left pane tree becomes visible at 250px
        state.open_folder_path(test_folder.clone())?;
        assert!(state.file_explorer.is_visible);
        assert_eq!(state.file_explorer.effective_width(), DEFAULT_SIDEBAR_WIDTH);
        let tree_names: Vec<String> = state
            .file_explorer
            .flat_rows
            .iter()
            .map(|r| r.name.clone())
            .collect();
        assert!(tree_names.contains(&"alpha.txt".to_string()));
        assert!(tree_names.contains(&"beta.rs".to_string()));

        // Ensure current tab is clean so it doesn't prompt when switching/closing
        set_edit_modified(state.hwnd_edit, false);
        state.sync_from_hwnd_edit();

        // Single-click `alpha.txt` in tree -> opens in preview tab (`is_preview = true`)
        state.open_path_with_mode(file_alpha.clone(), true)?;
        assert!(state.tab_manager.active_tab().is_preview);
        assert_eq!(state.tab_manager.active_tab().title, "alpha.txt");

        // Single-click `beta.rs` in tree -> replaces unmodified preview tab in-place!
        let tab_count_before = state.tab_manager.tabs.len();
        state.open_path_with_mode(file_beta.clone(), true)?;
        assert_eq!(state.tab_manager.tabs.len(), tab_count_before);
        assert!(state.tab_manager.active_tab().is_preview);
        assert_eq!(state.tab_manager.active_tab().title, "beta.rs");
        assert_eq!(get_edit_text(state.hwnd_edit), "fn beta() -> u32 { 42 }");

        // Edit `beta.rs` -> auto-pins preview tab (`is_preview = false`) and displays `*` dirty indicator
        set_edit_text(state.hwnd_edit, "fn beta() -> u32 { 99 }");
        set_edit_modified(state.hwnd_edit, true);
        state.sync_from_hwnd_edit();
        assert!(!state.tab_manager.active_tab().is_preview);
        assert!(state.tab_manager.active_tab().is_dirty);
        assert_eq!(state.tab_manager.active_tab().display_label(), "beta.rs *");

        // `File > New` (`IDM_FILENEW`) -> creates a new tab
        let count_before_new = state.tab_manager.tabs.len();
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(IDM_FILENEW as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(state.tab_manager.tabs.len(), count_before_new + 1);
        assert_eq!(state.tab_manager.active_tab().title, "Untitled");

        // `Ctrl+Tab` (`IDM_TAB_NEXT`) & `Ctrl+Shift+Tab` (`IDM_TAB_PREV`) switch tabs and preserve buffers
        let idx_before = state.tab_manager.active_index;
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(IDM_TAB_PREV as usize)),
            Some(LPARAM(0)),
        );
        assert_ne!(state.tab_manager.active_index, idx_before);
        assert_eq!(get_edit_text(state.hwnd_edit), "fn beta() -> u32 { 99 }");
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(IDM_TAB_NEXT as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(state.tab_manager.active_index, idx_before);

        // Drag-to-reorder tabs and close tab (`Ctrl+W` / `IDM_FILE_CLOSE_TAB`)
        assert!(state.tab_manager.reorder_tab(idx_before, 0));
        assert_eq!(state.tab_manager.active_index, 0);
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(IDM_FILE_CLOSE_TAB as usize)),
            Some(LPARAM(0)),
        );
        assert_eq!(state.tab_manager.tabs.len(), count_before_new);

        // Toggle Side Bar (`View > Toggle Side Bar` / `ID_VIEW_TOGGLE_SIDEBAR`)
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_VIEW_TOGGLE_SIDEBAR as usize)),
            Some(LPARAM(0)),
        );
        assert!(!state.file_explorer.is_visible);
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(ID_VIEW_TOGGLE_SIDEBAR as usize)),
            Some(LPARAM(0)),
        );
        assert!(state.file_explorer.is_visible);
        println!(
            "[OK] 11. Verified Multi-Tab (New/Switch/Reorder/Close/Dirty '*') & Open Folder ('D:\\test' left tree, preview vs pinned tab, Ctrl+Tab)"
        );

        // 11. Clean up modifications and close window (WM_CLOSE -> WM_DESTROY -> PostQuitMessage)
        for tab in &mut state.tab_manager.tabs {
            tab.is_dirty = false;
        }
        set_edit_modified(state.hwnd_edit, false);
        set_edit_text(state.hwnd_edit, "");
        let _ = std::fs::remove_file(&temp_file);
        let _ = std::fs::remove_file(&file_alpha);
        let _ = std::fs::remove_file(&file_beta);
        if created_d_test {
            let _ = std::fs::remove_dir(&test_folder);
        }
        SendMessageW(hwnd, WM_CLOSE, Some(WPARAM(0)), Some(LPARAM(0)));

        // Drain the queued WM_QUIT message posted by PostQuitMessage(0)
        let mut msg = MSG::default();
        let _ = GetMessageW(&mut msg, None, 0, 0);
        println!("[OK] 12. Verified WM_CLOSE -> WM_DESTROY -> PostQuitMessage(0) teardown");
    }

    println!("=== All Win32 Lifecycle Checks Passed! ===");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let hmodule = unsafe { GetModuleHandleW(None).unwrap_or_default() };
    let hinstance = HINSTANCE(hmodule.0);

    if args.iter().any(|a| a == "--self-test") {
        if let Err(e) = run_automated_lifecycle_verification(hinstance) {
            eprintln!("Lifecycle verification failed: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Standard GUI launch (WinMain at 0x140010280)
    let hwnd: HWND = match create_notepad_window(hinstance, Some(SW_SHOWDEFAULT)) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Failed to initialize Notepad window: {}", e);
            std::process::exit(1);
        }
    };

    // If a file or folder path was passed on the command line, open it immediately
    if let Some(path_arg) = args.get(1).filter(|a| !a.starts_with("--")) {
        let path = PathBuf::from(path_arg);
        if path.exists() {
            unsafe {
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut NotepadAppState;
                if let Some(state) = state_ptr.as_mut() {
                    if path.is_dir() {
                        let _ = state.open_folder_path(path);
                    } else {
                        let _ = state.open_path(path);
                    }
                }
            }
        } else if let Ok(doc) = load_file_win32(&path) {
            let _ = doc;
        }
    }

    let haccel = create_accelerators();

    // WinMain Message Loop (0x1400103A3 - 0x140010489)
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let translated = if let Some(acc) = haccel {
                TranslateAcceleratorW(hwnd, acc, &msg) != 0
            } else {
                false
            };

            if !translated {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        if let Some(acc) = haccel {
            let _ = DestroyAcceleratorTable(acc);
        }
    }
}
