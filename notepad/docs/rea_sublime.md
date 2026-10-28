# Reverse Engineering Analysis (REA): Sublime Text (`sublime_text.exe`)

## 1. Artifact Inventory & Evidence Ledger

| Artifact | Path | Format / Architecture | SHA-256 | REA Evidence ID |
| :--- | :--- | :--- | :--- | :--- |
| **Main Binary** | `C:\Program Files\Sublime Text\sublime_text.exe` | PE32+ (`x86_64`), 7,978,104 bytes, ImageBase `0x140000000` | `b3bba60bf219501996fe5349ec659eff25daf0072c0ae81f2ceac8b0c06788f2` | `ev_9bb934330d786a5fd786dae9a972df2e828a4adc400c9089a73345091870dfa2` |
| **Default Package** | `C:\Program Files\Sublime Text\Packages\Default.sublime-package` | ZIP archive (`Default` commands, menus, keymaps, mousemaps, settings) | `777b0c9c45370c84f67b05f8206f1274b4d83de69698fa79cafd7fc1bca39ec6` | `ev_488960856489ac22c4de1d52be7aa9efe19fec25f09d3b5ce515d6f94d2cf655` |
| **Default Theme** | `C:\Program Files\Sublime Text\Packages\Theme - Default.sublime-package` | ZIP archive (`Default.sublime-theme`, icons, adaptive UI rules) | `26d648cdfd1781e005e89dab96244a5a9a04d72300f993c0534efa00eb3a84f5` | `ev_bc56f6155015b8c7fe3c1fbedce29cc746711ce82fc8bba24f966cb49138b49f` |

### Binary Section Layout (`sublime_text.exe`)
- `.text`: `RVA = 0x00001000`, `VirtualSize = 0x005A5AA6`, `RawOffset = 0x00000400`
- `.rdata`: `RVA = 0x005A7000`, `VirtualSize = 0x001510B4`, `RawOffset = 0x005A6000`
- `.data`: `RVA = 0x006F9000`, `VirtualSize = 0x00052BA4`, `RawOffset = 0x006F7200`
- `.pdata`: `RVA = 0x0074C000`, `VirtualSize = 0x00032B68`, `RawOffset = 0x00737200`

---

## 2. Feature 1: Multi-Tab Architecture & Behavioral Specification

### 2.1 Binary Command Dispatch & Vtable Map
Sublime Text registers native `window_command` subclasses inside `.rdata` vtables where slot `+0x08` returns the command string name (`const char* name()`) and slot `+0x18` executes `run(args)`:

| Command Name | String VA | Vtable VA | `name()` VA | `run(args)` Function Bounds | Role |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `new_file` | `0x1406BFDC4` | `0x1405B9DE0` | `0x14005AD1C` | `[0x14005AD24..0x14005ADAA]` | Creates a new untitled buffer & tab in the active group |
| `close_file` / `close` | `0x1406BFDE0` | `0x1405BA080` | `0x14005B1A0` | `[0x14005B1A8..0x14005B28C]` | Closes active tab (`Ctrl+W`), prompting if dirty |
| `close_by_index` | `0x1406B392D` | `0x1405BA140` | `0x14005B290` | `[0x14005B298..0x14005B31E]` | Closes specific `(group, index)` tab from context menu / `X` button |
| `close_others_by_index` | `0x1406B3917` | `0x1405BA500` | `0x14005B8DC` | `[0x14005B8E6..0x14005B96C]` | Closes all tabs in group except `index` |
| `close_to_right_by_index` | `0x1406B38BD` | `0x1405BA380` | `0x14005B578` | `[0x14005B582..0x14005B613]` | Closes all tabs with tab index `> index` |
| `close_unmodified` | `0x1406C247E` | `0x1405BA5C0` | `0x14005BA54` | `[0x14005BA5E..0x14005BABA]` | Closes all clean (`!is_dirty`) tabs in group |
| `close_unmodified_to_right_by_index` | `0x1406B38D5` | `0x1405BA440` | `0x14005B748` | `[0x14005B752..0x14005B7E3]` | Closes clean tabs to the right of `index` |
| `next_view` | `0x1406B423F` | `0x1405BAE20` | `0x14005BD7C` | `[0x14005BD84..0x14005BE32]` | Linear tab cycle forward (`Ctrl+PageDown`) |
| `prev_view` | `0x1406B4235` | `0x1405BAEE0` | `0x14005BE34` | `[0x14005BE3C..0x14005BEEA]` | Linear tab cycle backward (`Ctrl+PageUp`) |
| `next_view_in_stack` | `0x1406BC7FA` | `0x1405BAFA0` | `0x14005BEEC` | `[0x14005BEF4..0x14005BEFC]` -> `0x1400DE282` | MRU / stack tab switch forward (`Ctrl+Tab`) |
| `prev_view_in_stack` | `0x1406BC7E7` | `0x1405BB060` | `0x14005BF00` | `[0x14005BF0A..0x14005BF12]` -> `0x1400DE282` | MRU / stack tab switch backward (`Ctrl+Shift+Tab`) |
| `select_by_index` | `0x1406B38F8` | `0x1405BBD80` | `0x14005CBFA` | `[0x14005CC02..0x14005CCA8]` | Direct 0-based tab selection (`Alt+1` .. `Alt+8`) |
| `select_last_tab` | `0x1406C40CD` | `0x1405BBE40` | `0x14005CCAA` | `[0x14005CCB2..0x14005CD3A]` | Selects the rightmost tab (`Alt+9`) |
| `reopen_last_file` | `0x1406BFDCD` | `0x1405B9C88` | `0x14005ABC4` | `[0x14005ABCC..0x14005AC4A]` -> `0x14005AB20` | Reopens most recently closed tab (`Ctrl+Shift+T`) |

### 2.2 New Tab Flow
1. **Triggers:**
   - Menu: `File > New File` (`Main.sublime-menu`, command `new_file`).
   - Shortcut: `Ctrl+N` (`Default (Windows).sublime-keymap`).
   - Tab Bar `+` button: `new_tab_button` (`"new_tab_button"` at `0x1406BA1E1`, tooltip `"New File"` at `0x1406C013B`) constructed inside `tabset_control` (`[0x1400E0F84..0x1400E1618]`, xref `0x1400E159D`), or double-clicking empty space on the tab bar.
2. **Insertion Position:**
   - Controlled by setting `"open_tabs_after_current"` (`0x1406B4D84`, default `true` in `Preferences.sublime-settings:655`), evaluated in `sheet_group` (`[0x140053352..0x140053584]` at `0x1400534FF`) and `edit_window` (`[0x140041BB4..0x140042DE0]` at `0x140042C73`).
   - When `true`, a newly created or opened tab is inserted immediately to the right of the currently active tab (`active_index + 1`), or appended if no tabs exist, and immediately becomes the active tab.
3. **Untitled Buffer Naming:**
   - Starts as `"untitled"` (or `"Untitled"`).
   - `Default/set_unsaved_view_name.py` inspects the first line of unsaved buffers on modification (`on_modified`) and dynamically mirrors the first line (up to 50 printable characters) as the tab title until saved.

### 2.3 Dirty Indicator (`*` / `•`)
1. **Tab Visual State:**
   - In `Default.sublime-theme` (`lines 752-776`), `tab_close_button` checks `attributes: ["!hover", "dirty"]` (and `["!hover", "selected", "dirty"]`).
   - When a buffer has unsaved edits (`is_dirty == true`), the tab displays a dirty indicator (`•` / `*`) in the tab header and appends `" •"` (`0x1406B2517` referenced at `0x140047CF6`) to the main window title bar (`"<filename> • - Sublime Text"`).
   - When the mouse hovers over the close button of a dirty tab, the indicator switches to the close `×` icon so the user can click to close.
   - Controlled by `"highlight_modified_tabs"` (`Preferences.sublime-settings:667`).

### 2.4 Closing Tabs (`X`, Middle-Click, `Ctrl+W`)
1. **Close Button (`X`):**
   - `tab_close_button` (`0x1406BB800`) hit-tested inside `tab_control` (`[0x14018EC59..0x14018ECF0]`). Controlled by `"show_tab_close_buttons"` (`0x1406BD860`, default `true`).
   - Clicking `X` closes that specific tab index even if it is not currently active.
2. **Middle-Click:**
   - Middle mouse button click (`WM_MBUTTONDOWN` / `button3`) directly over any `tab_control` closes that tab immediately.
3. **Keyboard (`Ctrl+W` / `Ctrl+F4`):**
   - Invokes `close` / `close_file` (`[0x14005B1A8..0x14005B28C]`), closing the active tab.
4. **Dirty Prompt & Last-Tab Behavior:**
   - If the closing tab has `is_dirty == true` and non-empty/unsaved changes, a modal Save / Don't Save / Cancel prompt is shown before discarding the buffer.
   - `"close_windows_when_empty"` (`0x1406B4F43`, default `false` on Windows in `Preferences.sublime-settings:676`): closing the last tab in a window keeps the window open with a fresh empty untitled buffer/state rather than exiting the process.

### 2.5 Drag to Reorder
1. **Mechanics (`[0x14018EC59..0x14018ECF0]` & `tabset_control`):**
   - Pressing `WM_LBUTTONDOWN` on a `tab_control` (outside `tab_close_button`) activates that tab immediately and records the drag origin index.
   - Moving the mouse horizontally (`WM_MOUSEMOVE` while `MK_LBUTTON` is held) past the midpoint of an adjacent tab swaps the dragged tab with the target tab index in the `TabManager` list while preserving `active_index` on the dragged tab.
   - Releasing `WM_LBUTTONUP` ends the drag capture.

### 2.6 Right-Click Context Menu (`Tab Context.sublime-menu`)
Right-clicking any tab opens a popup menu bound to that tab's `index` (from `Default/Tab Context.sublime-menu` & `Default/side_bar.py`):
- **Close Tab** (`close_by_index` with `{ "group": g, "index": i }`)
- **Close Other Tabs** (`close_others_by_index` with `{ "group": g, "index": i }`)
- **Close Tabs to the Right** (`close_to_right_by_index` with `{ "group": g, "index": i }`)
- **Close Unmodified Tabs** (`close_unmodified` with `{ "group": g }`)
- Separator
- **New File** (`new_file`)
- **Open File…** (`prompt_open_file`)

### 2.7 Tab Switching (`Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+PageDown/PageUp`)
- **`Ctrl+Tab` (`next_view_in_stack`) / `Ctrl+Shift+Tab` (`prev_view_in_stack`):**
  - Implemented at `0x14005BEF4` / `0x14005BF0A` -> `0x1400DE282`. Cycles through open tabs (MRU stack or sequential wrap-around).
- **`Ctrl+PageDown` (`next_view`) / `Ctrl+PageUp` (`prev_view`):**
  - Implemented at `0x14005BD84` / `0x14005BE3C`. Switches to `(active_index + 1) % len` and `(active_index + len - 1) % len`.
- **`Alt+1`..`Alt+8` (`select_by_index`) & `Alt+9` (`select_last_tab`):**
  - Jumps directly to tab index `0..7` or `len - 1`.

### 2.8 Multi-Buffer Model
- Each open tab (`Sheet` / `View` backed by a `TextBuffer`) owns its own independent state:
  - Unique tab/buffer identifier (`id`)
  - Optional file system path (`Option<PathBuf>`)
  - Display title (`String`)
  - Text content & encoding (`String`, `TextEncoding`)
  - Modification state (`is_dirty: bool`)
  - Transient/preview flag (`is_preview: bool`)
  - Cursor / selection / scroll position (`MultiCursorEditor`, `scroll_line: usize`)
- Switching tabs saves the outgoing tab's live editor buffer, cursor/selection state, and scroll offset, and loads the incoming tab's buffer, cursor/selection state, and scroll offset into the editor viewport.

---

## 3. Feature 2: `File > Open Folder` & Side Bar Behavioral Specification

### 3.1 Native Folder Picker (`prompt_open_folder`)
1. **Menu & Command:**
   - `Main.sublime-menu` line 11: `{ "command": "prompt_open_folder", "caption": "Open Folder…", "mnemonic": "F" }`.
   - String `"prompt_open_folder"` at `0x1406B8D71`, caption `"Open Folder"` at `0x1406B8DB2`, parameter `"initial_directory"` at `0x1406B3077`.
   - Command vtable at `0x1405DE420`, `run(args)` at `[0x1400FD514..0x1400FD5B7]` -> calls `0x1400FA934` with completion callback vtable `0x1405DE4D0` (`0x1400FD5B8`).
2. **Win32 COM Dialog (`[0x1401E858F..0x1401E89D8]`):**
   - Invokes `CoCreateInstance` (`IAT 0x1406E1318` at `0x1401E8679`) for `CLSID_FileOpenDialog` (`IFileOpenDialog`) configured with `FOS_PICKFOLDERS`, seeding the initial folder via `SHCreateItemFromParsingName` (`IAT 0x1406E1B28` at `0x1401E87D8`).
   - Upon folder selection, mounts the selected directory into the window's project/folder list and makes the left Side Bar visible automatically.

### 3.2 Post-Open Layout (`HSplit` + `sidebar_container` + `sidebar_tree`)
1. **Window Hierarchy (`edit_window` `[0x140041BB4..0x140042DE0]`):**
   - Constructs `"sidebar_container"` (`0x1406B8862` at `0x140042370`), `"sidebar_heading"` (`"FOLDERS"` header), and `"sidebar_tree"` (`0x1406C0B5D` at `0x14004229D`).
   - Placement controlled by `"sidebar_on_right"` (`0x1406B51FA`, default `false` = docked on the left).
   - Visibility toggled by `"toggle_side_bar"` (`0x1406B9386`, handler at `0x14004EBC4`, shortcut `Ctrl+K, Ctrl+B`, menu `View > Side Bar > Show/Hide Side Bar`).
2. **Resizable Splitter:**
   - A vertical splitter separates `sidebar_container` (default ~`250px` wide, clamped to a minimum width such as `140px` and maximum width `client_w - 200px`) from the right-hand editor area (`VBox [TabBar + Editor]`).
   - Hovering over the splitter changes the cursor to `IDC_SIZEWE` (`↔`); dragging adjusts the sidebar width and immediately reflows the TabBar, Line Number Gutter, and Editor viewports.

### 3.3 Left Pane Tree View, Expand/Collapse, and Icons
1. **Tree Rendering (`[0x140076172..0x140076686]` & `Default.sublime-theme:208-388`):**
   - Section header: `"FOLDERS"` (`sidebar_heading`, uppercase bold label).
   - Root folder row displayed at depth 0, initially expanded.
   - Each row (`tree_row`) has height ~`22-24px`, `indent: 12px` per nesting depth, `row_padding: [16, 2, 4, 2]`.
   - **Expand / Collapse:** Clicking any directory row toggles its expansion state in `expanded_paths` and redraws the visible flat row list.
2. **Icons & Visual Indicators:**
   - `disclosure_button_control` (`0x1406BB9AE` at `0x1401AA0C6`): chevron `▸` (collapsed) / `▾` (expanded) next to directories.
   - `icon_folder` (`0x1406B8D65` at `0x14007642D`): folder glyph (`📁` / drawn folder icon) next to directories.
   - `icon_file_type` (`0x1406BEF3A` at `0x14007637A`): file glyph (`📄` / file badge) next to files.
   - Selection & Hover highlights: `tree_row` renders a distinct highlight background when `attributes: ["selected"]` (matches active file) or `attributes: ["hover"]`.

### 3.4 Single-Click Preview Tab vs Double-Click Pinned Tab
1. **Binary Trace (`[0x140077C5E..0x140077D01]`):**
   - Reads `"preview_on_click"` (`0x1406BC72E` at `0x140077C7C`, default `true` in `Preferences.sublime-settings:862`).
2. **Single-Click on File in Tree (`Preview Mode`):**
   - If the clicked file is **already open** in any tab (preview or pinned), that tab is simply activated (`active_index = idx`).
   - Otherwise, if an existing **unmodified preview tab** (`is_preview == true && !is_dirty`) exists in the tab bar, its contents and path are **replaced in-place** by the newly clicked file.
   - If no preview tab exists, a new tab is opened with `is_preview = true`.
   - Preview tabs render their title in *italic* font (`Default.sublime-theme:736`: `"class": "tab_label", "attributes": ["transient"], "font.italic": true`).
3. **Double-Click on File in Tree (`Pin Tab`):**
   - Double-clicking a file in the tree opens it (if not already open) and pins it (`is_preview = false`), rendering its title in normal (non-italic) font so subsequent single-clicks on other files will not replace it.
4. **Auto-Pin on Edit:**
   - Typing or modifying text inside a preview tab (`EN_CHANGE` / buffer edit) automatically promotes the tab from transient preview to pinned (`is_preview = false`, `is_dirty = true`).

### 3.5 File Watcher Auto-Refresh (`ReadDirectoryChangesW`)
- In `sublime_text.exe`, `[0x1401EB27F..0x1401EBA13]` invokes Win32 `ReadDirectoryChangesW` (`IAT 0x1406E17F0` at `0x1401EB867`) on mounted project directories.
- When files or directories are created, renamed, or deleted on disk inside the opened folder, the background watcher notifies the UI thread, which refreshes the in-memory `FileExplorer` tree while preserving the user's `expanded_paths` state and scroll position.

### 3.6 Sidebar Context Menu (`Side Bar.sublime-menu` & `Side Bar Mount Point.sublime-menu`)
Right-clicking a node in the sidebar tree shows context actions:
- **Open File** (opens/pins selected file)
- **New File…** (creates a new file in the target directory and opens it in a pinned tab)
- **Copy Path** (copies the selected node's full path to the Windows clipboard)
- **Refresh Folder** (re-scans the root directory tree)
- **Close Folder** (unmounts the folder and hides the sidebar, matching `Remove Folder from Project`)

### 3.7 Empty Folder Handling & Large Folder Performance
1. **Empty Folder Handling:**
   - Expanding an empty root folder or empty subdirectory succeeds cleanly without error, displaying the expanded disclosure chevron (`▾`) with zero child rows (or an `(empty folder)` indicator row when the root folder itself has no entries).
2. **Large Folder Performance (`[0x140071E22..0x140072511]`):**
   - Applies `"folder_exclude_patterns"` (`0x1406B6829`): skips heavy/metadata directories (`".git"`, `".svn"`, `".hg"`, `"CVS"`, `"target"`, `"node_modules"`, `".idea"`, `".vscode"`) and binary/system artifacts (`"*.dll"`, `"*.exe"`, `"*.pdb"`, `"*.obj"`, `"*.o"`, `"*.DS_Store"`).
   - Sorts directories first (case-insensitive alphabetical), followed by files (case-insensitive alphabetical).
   - Caps traversal depth/entry count and supports lazy child loading on directory expansion so opening a large repository never blocks the UI thread.
