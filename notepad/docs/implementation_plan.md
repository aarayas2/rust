# Implementation Plan: Sublime Text Multi-Tab & Open Folder Upgrade

## 1. Audit of Destination Codebase (`D:\github\rust\notepad`)

### 1.1 GUI Framework & Rendering Stack
- **Crate:** `notepad_win32` (`Cargo.toml`, Rust 2021 edition).
- **GUI Backend:** Native Win32 API via `windows = "0.62"` (`Win32_UI_WindowsAndMessaging`, `Win32_Graphics_Gdi`, `Win32_UI_Controls`, `Win32_UI_Input_KeyboardAndMouse`, `Win32_UI_Com_Dialogs`, `Win32_System_DataExchange`, etc.).
- **Window Architecture (`src/window.rs`):**
  - `hwnd_main`: Top-level `NotepadWin32Class` window hosting the menu bar, custom GDI line-number gutter, and child controls.
  - `hwnd_edit`: Subclassed Win32 `"Edit"` control (`ES_MULTILINE | ES_AUTOVSCROLL | ES_AUTOHSCROLL | ES_WANTRETURN | WS_VSCROLL | WS_HSCROLL`) with custom GDI overlay rendering for multi-cursor carets and rectangular block selection (`MultiCursorEditor`).
  - `hwnd_status`: Win32 `"STATIC"` control at the bottom edge displaying line/column, cursor count, zoom %, encoding, and theme name.

### 1.2 Current Application State (`NotepadAppState` in `src/window.rs`)
Currently, `NotepadAppState` is strictly **single-document**:
- `current_file: Option<PathBuf>`
- `encoding: TextEncoding`
- `multi_cursor: MultiCursorEditor`
- Text buffer resides solely inside `hwnd_edit` (`get_edit_text()` / `set_edit_text()`) with a single Win32 `EM_GETMODIFY` / `EM_SETMODIFY` flag (`is_edit_modified()` / `set_edit_modified()`).
- `resize_children(width, height)` positions only `[Gutter | hwnd_edit]` above `hwnd_status`.

---

## 2. Gap Analysis (DST vs. Sublime Text REA Specification)

| Capability | Sublime Text (`SRC`) | Current Notepad (`DST`) | Required Upgrade |
| :--- | :--- | :--- | :--- |
| **Document Model** | Multi-buffer (`SheetGroup` + `Sheet`/`View` list, MRU stack) | Single buffer in `hwnd_edit`, single `current_file: Option<PathBuf>` | Introduce `Tab` & `TabManager` (`src/tabs.rs`) syncing active buffer with `hwnd_edit` |
| **Tab Bar UI** | `tabset_control` (`32px` height), `tab_control` with active indicator, italic preview label, dirty `*`/`•`, `×` close button, `+` new tab button | None | Custom double-buffered GDI Tab Bar at top of editor pane (`y = 0..32`) |
| **Tab Interactions** | Click to switch, `×` / middle-click / `Ctrl+W` to close, drag-to-reorder, right-click menu (`Close Others`, `Close to Right`, `Close Unmodified`), `Ctrl+Tab` / `Ctrl+Shift+Tab` | None | Mouse hit-testing, drag-reorder tracking, Win32 popup context menu, keyboard accelerators |
| **File > Open Folder** | `prompt_open_folder` native folder picker -> opens left `sidebar_container` + `sidebar_tree` (`250px` resizable splitter) | Only `File > Open...` (single file) | Add `File > Open Folder...` (`Ctrl+Shift+O`) via `rfd` + programmatic folder loader |
| **Side Bar Tree View** | `FOLDERS` header, expand/collapse directories (`▸`/`▾`), folder/file icons, single-click = preview tab, double-click = pinned tab, edit = auto-pin | None | Introduce `FileExplorer` & `FileNode` (`src/file_explorer.rs`) with GDI tree renderer & splitter |
| **File Watcher & Sidebar Toggle** | `ReadDirectoryChangesW` live tree refresh, `View > Toggle Side Bar` (`Ctrl+K, Ctrl+B`), exclude patterns | None | Add `notify` file watcher polled on Win32 timer + `View > Toggle Side Bar` menu/shortcut |

---

## 3. Technical Design

### 3.1 Crate Additions (`Cargo.toml`)
Add the following dependencies to `Cargo.toml`:
- `walkdir = "2"` — Fast directory traversal with entry filtering for `FileExplorer`.
- `notify = "6"` — Cross-platform file system watcher (uses `ReadDirectoryChangesW` on Windows) for automatic sidebar tree refresh.
- `rfd = "0.15"` — Native Windows `IFileOpenDialog` (`FOS_PICKFOLDERS`) folder picker for `File > Open Folder...`.

### 3.2 Multi-Tab Data Structures (`src/tabs.rs`)

```rust
use std::path::PathBuf;
use crate::file_io::TextEncoding;
use crate::window::MultiCursorEditor;

#[derive(Debug, Clone)]
pub struct Tab {
    pub id: u64,
    pub path: Option<PathBuf>,
    pub title: String,
    pub content: String,
    pub encoding: TextEncoding,
    pub is_dirty: bool,
    pub is_preview: bool,
    pub cursor_pos: (usize, usize),
    pub scroll_line: i32,
    pub multi_cursor: MultiCursorEditor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabHitTarget {
    TabBody(usize),
    TabClose(usize),
    NewTabButton,
    EmptyBar,
}

#[derive(Debug, Clone)]
pub struct TabRect {
    pub index: usize,
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub close_left: i32,
    pub close_top: i32,
    pub close_right: i32,
    pub close_bottom: i32,
}

pub struct TabManager {
    pub tabs: Vec<Tab>,
    pub active_index: usize,
    pub mru_stack: Vec<u64>,
    pub next_id: u64,
    pub hover_tab: Option<usize>,
    pub hover_close: Option<usize>,
    pub hover_new_btn: bool,
    pub dragging_tab: Option<usize>,
    pub drag_start_x: i32,
}
```

#### Key `TabManager` Operations
- `new_tab(&mut self) -> usize`: Inserts a new untitled pinned tab right after `active_index` (matching Sublime's `open_tabs_after_current = true`), activates it, and records its ID in `mru_stack`.
- `open_file_in_tab(&mut self, path: PathBuf, content: String, encoding: TextEncoding, preview: bool) -> (usize, bool)`:
  - If `path` is already open in `tabs`, switches to that tab; if `preview == false`, also promotes `is_preview = false`.
  - If `preview == true`, looks for an existing unmodified preview tab (`t.is_preview && !t.is_dirty`); if found, replaces it in-place and activates it.
  - Otherwise, creates a new `Tab` with `is_preview = preview` after `active_index` and activates it.
- `pin_tab(&mut self, index: usize)`: Sets `tabs[index].is_preview = false`.
- `mark_active_dirty(&mut self, dirty: bool)`: Sets `is_dirty = dirty` on the active tab; if `dirty == true`, automatically pins the tab (`is_preview = false`) and updates untitled tab title from first line if applicable.
- `close_tab(&mut self, index: usize) -> bool`: Removes tab at `index`. If `tabs` becomes empty, creates a fresh untitled tab so `tabs.len() >= 1` always holds. Adjusts `active_index` seamlessly.
- `close_others(&mut self, keep_index: usize)` / `close_to_right(&mut self, from_index: usize)` / `close_unmodified(&mut self)`: Batch tab close operations matching `Tab Context.sublime-menu`.
- `reorder_tab(&mut self, from: usize, to: usize)`: Moves tab from `from` to `to` during horizontal drag and keeps `active_index = to`.
- `next_tab(&mut self)` / `prev_tab(&mut self)`: Cycles `(active_index + 1) % len` and `(active_index + len - 1) % len` (`Ctrl+Tab` / `Ctrl+Shift+Tab`).

### 3.3 Open Folder & Left Pane Tree Data Structures (`src/file_explorer.rs`)

```rust
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use notify::{RecommendedWatcher, Event};

#[derive(Debug, Clone)]
pub enum FileNode {
    File {
        name: String,
        path: PathBuf,
    },
    Dir {
        name: String,
        path: PathBuf,
        children: Vec<FileNode>,
        is_loaded: bool,
    },
}

#[derive(Debug, Clone)]
pub struct FlatTreeRow {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub is_expanded: bool,
    pub is_empty_dir: bool,
}

pub struct FileExplorer {
    pub root_path: Option<PathBuf>,
    pub root_node: Option<FileNode>,
    pub is_visible: bool,
    pub expanded_paths: HashSet<PathBuf>,
    pub flat_rows: Vec<FlatTreeRow>,
    pub selected_path: Option<PathBuf>,
    pub hover_row: Option<usize>,
    pub width: i32,
    pub is_resizing: bool,
    pub scroll_y: i32,
    pub row_height: i32,
    pub header_height: i32,
    watcher: Option<RecommendedWatcher>,
    watch_rx: Option<Receiver<notify::Result<Event>>>,
}
```

#### Key `FileExplorer` Operations
- `open_folder(&mut self, path: PathBuf) -> Result<(), String>`:
  - Canonicalizes/normalizes `path`, sets `root_path = Some(path.clone())`, `is_visible = true`, inserts `path` into `expanded_paths`, loads directory tree via `walkdir` (filtering excluded directories like `.git`, `target`, `node_modules` and sorting directories before files), rebuilds `flat_rows`, and starts a `notify::RecommendedWatcher` on `path`.
- `toggle_expand(&mut self, path: &Path)`:
  - Toggles `path` in `expanded_paths`, lazily loads children if `Dir { is_loaded: false }`, and rebuilds `flat_rows`.
- `refresh(&mut self)`:
  - Re-scans `root_path` while preserving `expanded_paths`, `selected_path`, and `scroll_y`.
- `poll_watcher(&mut self) -> bool`:
  - Drains `watch_rx`; if any filesystem events arrived, calls `refresh()` and returns `true` so the window invalidates and repaints the sidebar.

### 3.4 Viewport Layout (`HSplit` + `VBox`)

```
+-----------------------------------------------------------------------+
| Menu Bar (File | Edit | Format | View | Help)                         |
+----------------------+------------------------------------------------+
| FOLDERS        (28px)| [Tab 1 * x] [preview.rs x] [+]     (TabBar 32px)|
| v project_root       +------+-----------------------------------------+
|   > src              |    1 | fn main() {                             |
|     Cargo.toml       |    2 |     println!("Hello");                  |
|     README.md        |    3 | }                                       |
|                      |      |                                         |
|  FileExplorer        |Gutter| Editor (hwnd_edit)                      |
|  (250px resizable)   |(48px)|                                         |
|                      |      |                                         |
+----------------------+------+-----------------------------------------+
| Ln 1, Col 1 | 2 Tabs | Folder: project_root | 100% | UTF-8 | Dark     |
+-----------------------------------------------------------------------+
```

- **Horizontal Split (`HSplit`):**
  - When `file_explorer.is_visible && file_explorer.root_path.is_some()`:
    - Left Pane (`FileExplorer`): `x = 0 .. sidebar_w` (default `250px`, clamped `140px .. client_w - 200px`).
    - Splitter Hit Zone: `x = sidebar_w - 3 .. sidebar_w + 3` (`IDC_SIZEWE` cursor).
    - Right Pane (`VBox [TabBar + Editor]`): starts at `right_x = sidebar_w`.
  - When sidebar is hidden (`!file_explorer.is_visible`):
    - `right_x = 0`, `sidebar_w = 0`.
- **Vertical Split inside Right Pane (`VBox`):**
  - `TabBar`: `x = right_x .. client_w`, `y = 0 .. TAB_BAR_HEIGHT (32px)`.
  - `Gutter`: `x = right_x .. right_x + gutter_w`, `y = TAB_BAR_HEIGHT .. client_h - status_h`.
  - `hwnd_edit`: `x = right_x + gutter_w .. client_w`, `y = TAB_BAR_HEIGHT .. client_h - status_h`.
- **Bottom Status Bar:**
  - `hwnd_status`: `x = 0 .. client_w`, `y = client_h - status_h .. client_h`.

---

## 4. Files to Create or Modify

| File Path | Action | Responsibility |
| :--- | :--- | :--- |
| `docs/rea_sublime.md` | **Create** | Phase 1 REA behavioral and binary trace of `sublime_text.exe` |
| `docs/implementation_plan.md` | **Create** | Phase 2 architecture audit, gap analysis, and implementation design |
| `Cargo.toml` | **Modify** | Add `walkdir`, `notify`, `rfd`, and `Win32_UI_Shell` / `Win32_System_Com` features |
| `src/lib.rs` | **Modify** | Export new `pub mod tabs;` and `pub mod file_explorer;` modules |
| `src/tabs.rs` | **Create** | `Tab`, `TabManager`, tab bar layout calculation, hit-testing, GDI rendering, MRU/reorder/close logic, unit tests |
| `src/file_explorer.rs` | **Create** | `FileNode`, `FileExplorer`, `walkdir` tree loader with exclude rules, `notify` watcher, GDI sidebar renderer, hit-testing, unit tests |
| `src/menu.rs` | **Modify** | Add menu IDs and items: `IDM_FILE_OPEN_FOLDER`, `IDM_FILE_CLOSE_TAB`, `IDM_TAB_NEXT`, `IDM_TAB_PREV`, `IDM_VIEW_TOGGLE_SIDEBAR`, plus context menu IDs for tabs and sidebar |
| `src/theme.rs` | **Modify** | Add theme colors for Tab Bar (bg, active tab, inactive tab, hover tab, tab border, accent line, dirty dot) and Sidebar (bg, header, selected row, hover row, splitter) |
| `src/window.rs` | **Modify** | Integrate `TabManager` and `FileExplorer` into `NotepadAppState`; update `resize_children` for `HSplit`/`VBox`; wire GDI painting, mouse events (click, double-click, middle-click, drag-reorder, splitter drag, right-click context menus, scroll wheel), keyboard shortcuts (`Ctrl+W`, `Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+Shift+O`, `Ctrl+B`), and watcher timer |
| `src/main.rs` | **Modify** | Support opening a folder or file path from CLI arguments; extend `--self-test` to verify multi-tab creation, switching, reordering, closing, `Open Folder`, tree single-click preview vs double-click pin, dirty `*` indicator, and `Ctrl+Tab` |
