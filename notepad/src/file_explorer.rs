//! Sublime Text-style Left Pane Folder Tree (`FileExplorer`, `FileNode`) & File Watcher.
//!
//! # Reverse Engineering Reference (`C:\Program Files\Sublime Text\sublime_text.exe`)
//! - `prompt_open_folder` (`0x1400FD514`, vtable `0x1405DE420`): invokes native Win32
//!   folder picker (`0x1401E858F`) and mounts folder into `sidebar_container` (`0x1406B8862`)
//!   + `sidebar_tree` (`0x1406C0B5D`).
//! - `toggle_side_bar` (`0x14004EBC4`, `Ctrl+K, Ctrl+B`): toggles `sidebar_container` visibility.
//! - `preview_on_click` (`0x1406BC72E` at `0x140077C7C`): single-click opens transient preview
//!   tab (`is_preview = true`); double-click pins tab (`is_preview = false`).
//! - Folder indexing & exclusions (`0x140071E22..0x140072511`): applies `folder_exclude_patterns`
//!   (`0x1406B6829`: `[".svn", ".git", ".hg", "CVS", ".Trash"]`) and `file_exclude_patterns`.
//! - Live directory watching (`0x1401EB27F..0x1401EBA13`): uses `ReadDirectoryChangesW`
//!   (`IAT 0x1406E17F0` at `0x1401EB867`) to refresh the sidebar tree on filesystem mutations.

use crate::theme::{RgbColor, Theme};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use walkdir::WalkDir;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen, CreateSolidBrush,
    DeleteDC, DeleteObject, DrawTextW, FillRect, LineTo, MoveToEx, SelectObject, SetBkMode,
    SetTextColor, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FONT_CHARSET,
    FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION, FONT_QUALITY, HDC, PS_SOLID, SRCCOPY, TRANSPARENT,
};

/// Default width of the left `FileExplorer` pane in pixels (`250px`).
pub const DEFAULT_SIDEBAR_WIDTH: i32 = 250;
/// Minimum resizable width of the left `FileExplorer` pane in pixels.
pub const MIN_SIDEBAR_WIDTH: i32 = 140;
/// Half-width hit tolerance (in pixels) around the sidebar splitter border.
pub const SPLITTER_HIT_HALF_WIDTH: i32 = 4;
/// Height of the `"FOLDERS"` header row in the sidebar.
pub const SIDEBAR_HEADER_HEIGHT: i32 = 28;
/// Height of each tree item row in the sidebar (`Default.sublime-theme` row height).
pub const SIDEBAR_ROW_HEIGHT: i32 = 24;
/// Maximum entries loaded per directory to preserve UI responsiveness on huge folders.
pub const MAX_DIR_ENTRIES: usize = 500;

/// Excluded directory names modeled after Sublime Text's `folder_exclude_patterns` (`0x1406B6829`).
const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "CVS",
    ".Trash",
    "target",
    "node_modules",
    ".idea",
    ".vscode",
];

/// Excluded file extensions modeled after Sublime Text's `file_exclude_patterns` / `binary_file_patterns`.
const EXCLUDED_EXTENSIONS: &[&str] = &[
    "pyc", "pyo", "exe", "dll", "obj", "o", "a", "lib", "so", "dylib", "ncb", "sdf", "suo", "pdb",
    "idb", "DS_Store", "class", "psd", "db", "sublime-workspace",
];

/// Hierarchical filesystem node inside `FileExplorer`.
#[derive(Debug, Clone)]
pub enum FileNode {
    /// Regular file leaf node.
    File { name: String, path: PathBuf },
    /// Directory node containing sorted child `FileNode`s (directories first, then files).
    Dir {
        name: String,
        path: PathBuf,
        children: Vec<FileNode>,
        is_loaded: bool,
    },
}

impl FileNode {
    /// Returns the display name of this node.
    pub fn name(&self) -> &str {
        match self {
            FileNode::File { name, .. } | FileNode::Dir { name, .. } => name,
        }
    }

    /// Returns the filesystem path of this node.
    pub fn path(&self) -> &Path {
        match self {
            FileNode::File { path, .. } | FileNode::Dir { path, .. } => path,
        }
    }

    /// Returns `true` if this node is a directory.
    pub fn is_dir(&self) -> bool {
        matches!(self, FileNode::Dir { .. })
    }
}

/// Flattened visible tree row for fast O(1) GDI rendering and mouse row hit-testing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatTreeRow {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub is_expanded: bool,
    pub is_empty_placeholder: bool,
}

/// Hit-test target when clicking or hovering inside the left `FileExplorer` pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarHitTarget {
    /// Mouse is on the resizable vertical splitter bar (`x ≈ sidebar_width`).
    Splitter,
    /// Mouse is on the `"FOLDERS"` header bar.
    Header,
    /// Mouse is on row `index` in `flat_rows`.
    Row(usize),
    /// Mouse is in the empty area below the tree rows.
    EmptySpace,
}

/// State and controller for the left-pane Folder Explorer (`File > Open Folder...`).
pub struct FileExplorer {
    /// Root folder path currently mounted (`None` before `Open Folder`).
    pub root_path: Option<PathBuf>,
    /// Root directory tree node.
    pub root_node: Option<FileNode>,
    /// Whether the left sidebar pane is visible (`View > Toggle Side Bar`).
    pub is_visible: bool,
    /// Set of normalized paths of currently expanded directories.
    pub expanded_paths: HashSet<PathBuf>,
    /// Flattened visible rows corresponding to the current `expanded_paths` state.
    pub flat_rows: Vec<FlatTreeRow>,
    /// Currently selected node path in the tree.
    pub selected_path: Option<PathBuf>,
    /// Currently hovered row index in `flat_rows`.
    pub hover_row: Option<usize>,
    /// Current pixel width of the left sidebar pane (default `250px`).
    pub width: i32,
    /// Whether the user is actively dragging the vertical splitter.
    pub is_resizing: bool,
    /// Vertical scroll offset in rows.
    pub scroll_row: usize,
    /// Active `notify` filesystem watcher for auto-refreshing the folder tree.
    watcher: Option<RecommendedWatcher>,
    /// Receiver channel for filesystem change events from `watcher`.
    watch_rx: Option<Receiver<notify::Result<Event>>>,
}

impl Default for FileExplorer {
    fn default() -> Self {
        Self::new()
    }
}

impl FileExplorer {
    /// Creates an initially hidden `FileExplorer` with default width `250px`.
    pub fn new() -> Self {
        Self {
            root_path: None,
            root_node: None,
            is_visible: false,
            expanded_paths: HashSet::new(),
            flat_rows: Vec::new(),
            selected_path: None,
            hover_row: None,
            width: DEFAULT_SIDEBAR_WIDTH,
            is_resizing: false,
            scroll_row: 0,
            watcher: None,
            watch_rx: None,
        }
    }

    /// Returns the effective pixel width occupied by the sidebar (`0` when hidden or no folder open).
    pub fn effective_width(&self) -> i32 {
        if self.is_visible && self.root_path.is_some() {
            self.width.max(MIN_SIDEBAR_WIDTH)
        } else {
            0
        }
    }

    /// Opens `folder_path` as the active workspace folder, populates the `FileNode` tree,
    /// expands the root node, shows the left pane, and starts the `notify` directory watcher.
    pub fn open_folder(&mut self, folder_path: PathBuf) -> Result<(), String> {
        if !folder_path.exists() {
            return Err(format!("Folder does not exist: {}", folder_path.display()));
        }
        if !folder_path.is_dir() {
            return Err(format!("Path is not a directory: {}", folder_path.display()));
        }

        self.root_path = Some(folder_path.clone());
        self.is_visible = true;
        self.scroll_row = 0;
        self.expanded_paths.clear();
        self.expanded_paths.insert(folder_path.clone());

        self.root_node = Some(Self::load_dir_node(&folder_path, &self.expanded_paths));
        self.rebuild_flat_rows();
        self.start_watcher(&folder_path);
        Ok(())
    }

    /// Closes the currently open folder, stops the file watcher, and hides the sidebar.
    pub fn close_folder(&mut self) {
        self.watcher = None;
        self.watch_rx = None;
        self.root_path = None;
        self.root_node = None;
        self.flat_rows.clear();
        self.expanded_paths.clear();
        self.selected_path = None;
        self.hover_row = None;
        self.is_visible = false;
    }

    /// Toggles sidebar visibility (`View > Toggle Side Bar` / `Ctrl+K, Ctrl+B`).
    /// Returns the new visibility state.
    pub fn toggle_visibility(&mut self) -> bool {
        if self.root_path.is_some() {
            self.is_visible = !self.is_visible;
        } else {
            self.is_visible = false;
        }
        self.is_visible
    }

    /// Toggles the expanded/collapsed state of a directory at `dir_path` and refreshes `flat_rows`.
    pub fn toggle_expand(&mut self, dir_path: &Path) {
        let key = dir_path.to_path_buf();
        if self.expanded_paths.contains(&key) {
            self.expanded_paths.remove(&key);
        } else {
            self.expanded_paths.insert(key);
        }
        if let Some(root) = self.root_path.clone() {
            self.root_node = Some(Self::load_dir_node(&root, &self.expanded_paths));
            self.rebuild_flat_rows();
        }
    }

    /// Re-scans the mounted `root_path` while preserving `expanded_paths` and `selected_path`.
    pub fn refresh(&mut self) {
        if let Some(root) = self.root_path.clone() {
            if root.is_dir() {
                self.root_node = Some(Self::load_dir_node(&root, &self.expanded_paths));
                self.rebuild_flat_rows();
            }
        }
    }

    /// Polls the background `notify` watcher channel without blocking.
    /// If any filesystem events occurred inside the watched folder, refreshes the tree and returns `true`.
    pub fn poll_watcher(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.watch_rx {
            while let Ok(res) = rx.try_recv() {
                if res.is_ok() {
                    changed = true;
                }
            }
        }
        if changed {
            self.refresh();
        }
        changed
    }

    /// Starts a `notify::RecommendedWatcher` (`ReadDirectoryChangesW` on Windows) on `folder_path`.
    fn start_watcher(&mut self, folder_path: &Path) {
        self.watcher = None;
        self.watch_rx = None;

        let (tx, rx) = channel();
        if let Ok(mut watcher) = RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            notify::Config::default(),
        ) {
            if watcher
                .watch(folder_path, RecursiveMode::Recursive)
                .is_ok()
            {
                self.watcher = Some(watcher);
                self.watch_rx = Some(rx);
            }
        }
    }

    /// Recursively loads a directory into a `FileNode::Dir`, loading children for directories
    /// that are in `expanded_paths` using `walkdir` (depth 1 per expanded level) and filtering
    /// Sublime exclude patterns.
    fn load_dir_node(dir_path: &Path, expanded_paths: &HashSet<PathBuf>) -> FileNode {
        let name = dir_path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| dir_path.to_str().unwrap_or("Folder"))
            .to_string();

        let is_expanded = expanded_paths.contains(dir_path);
        if !is_expanded {
            return FileNode::Dir {
                name,
                path: dir_path.to_path_buf(),
                children: Vec::new(),
                is_loaded: false,
            };
        }

        let mut dirs = Vec::new();
        let mut files = Vec::new();

        let walker = WalkDir::new(dir_path)
            .min_depth(1)
            .max_depth(1)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !Self::is_excluded_entry(e.path()));

        for (count, entry) in walker.flatten().enumerate() {
            if count >= MAX_DIR_ENTRIES {
                break;
            }
            let path = entry.path().to_path_buf();
            let entry_name = entry.file_name().to_string_lossy().to_string();
            if entry.file_type().is_dir() {
                dirs.push(Self::load_dir_node(&path, expanded_paths));
            } else if entry.file_type().is_file() {
                files.push(FileNode::File {
                    name: entry_name,
                    path,
                });
            }
        }

        dirs.sort_by(|a, b| a.name().to_lowercase().cmp(&b.name().to_lowercase()));
        files.sort_by(|a, b| a.name().to_lowercase().cmp(&b.name().to_lowercase()));
        dirs.extend(files);

        FileNode::Dir {
            name,
            path: dir_path.to_path_buf(),
            children: dirs,
            is_loaded: true,
        }
    }

    /// Checks whether `path` matches Sublime Text's `folder_exclude_patterns` or `file_exclude_patterns`.
    pub fn is_excluded_entry(path: &Path) -> bool {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        if path.is_dir() {
            return EXCLUDED_DIRS
                .iter()
                .any(|excluded| name.eq_ignore_ascii_case(excluded));
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if EXCLUDED_EXTENSIONS
                .iter()
                .any(|excluded| ext.eq_ignore_ascii_case(excluded))
            {
                return true;
            }
        }
        false
    }

    /// Rebuilds the flattened visible row list (`flat_rows`) from `root_node` and `expanded_paths`.
    pub fn rebuild_flat_rows(&mut self) {
        self.flat_rows.clear();
        if let Some(root) = &self.root_node {
            Self::flatten_node(root, 0, &self.expanded_paths, &mut self.flat_rows);
        }
        if self.scroll_row >= self.flat_rows.len() {
            self.scroll_row = 0;
        }
    }

    fn flatten_node(
        node: &FileNode,
        depth: usize,
        expanded_paths: &HashSet<PathBuf>,
        out: &mut Vec<FlatTreeRow>,
    ) {
        match node {
            FileNode::File { name, path } => {
                out.push(FlatTreeRow {
                    path: path.clone(),
                    name: name.clone(),
                    depth,
                    is_dir: false,
                    is_expanded: false,
                    is_empty_placeholder: false,
                });
            }
            FileNode::Dir {
                name,
                path,
                children,
                ..
            } => {
                let is_expanded = expanded_paths.contains(path);
                out.push(FlatTreeRow {
                    path: path.clone(),
                    name: name.clone(),
                    depth,
                    is_dir: true,
                    is_expanded,
                    is_empty_placeholder: false,
                });
                if is_expanded {
                    if children.is_empty() {
                        out.push(FlatTreeRow {
                            path: path.clone(),
                            name: String::from("(empty folder)"),
                            depth: depth + 1,
                            is_dir: false,
                            is_expanded: false,
                            is_empty_placeholder: true,
                        });
                    } else {
                        for child in children {
                            Self::flatten_node(child, depth + 1, expanded_paths, out);
                        }
                    }
                }
            }
        }
    }

    /// Scrolls the sidebar vertically by `delta_rows` (`WM_MOUSEWHEEL`).
    pub fn scroll_by(&mut self, delta_rows: i32) {
        if self.flat_rows.is_empty() {
            self.scroll_row = 0;
            return;
        }
        let max_scroll = self.flat_rows.len().saturating_sub(1) as i32;
        let next = (self.scroll_row as i32 + delta_rows).clamp(0, max_scroll);
        self.scroll_row = next as usize;
    }

    /// Hit-tests client coordinates `(x, y)` against the sidebar and splitter within `[0..sidebar_w, 0..pane_height]`.
    pub fn hit_test(&self, x: i32, y: i32, pane_height: i32) -> Option<SidebarHitTarget> {
        let w = self.effective_width();
        if w <= 0 || y < 0 || y >= pane_height {
            return None;
        }
        if (x - w).abs() <= SPLITTER_HIT_HALF_WIDTH {
            return Some(SidebarHitTarget::Splitter);
        }
        if x < 0 || x >= w {
            return None;
        }
        if y < SIDEBAR_HEADER_HEIGHT {
            return Some(SidebarHitTarget::Header);
        }
        let rel_y = y - SIDEBAR_HEADER_HEIGHT;
        let visible_idx = (rel_y / SIDEBAR_ROW_HEIGHT) as usize;
        let row_idx = self.scroll_row + visible_idx;
        if row_idx < self.flat_rows.len() {
            Some(SidebarHitTarget::Row(row_idx))
        } else {
            Some(SidebarHitTarget::EmptySpace)
        }
    }

    /// Renders the left `FileExplorer` pane and vertical splitter onto `hdc` inside
    /// `[0, 0, self.effective_width(), pane_height]`.
    pub fn render_gdi(&self, hdc: HDC, pane_height: i32, theme: &Theme) {
        let w = self.effective_width();
        if w <= 0 || pane_height <= 0 {
            return;
        }

        let sidebar_bg = if theme.is_dark {
            theme.background.blend(RgbColor::new(0, 0, 0), 22, 100)
        } else {
            theme.background.blend(RgbColor::new(0, 0, 0), 7, 100)
        };
        let header_bg = if theme.is_dark {
            theme.background.blend(RgbColor::new(0, 0, 0), 30, 100)
        } else {
            theme.background.blend(RgbColor::new(0, 0, 0), 11, 100)
        };
        let selected_bg = theme.selection;
        let hover_bg = theme.line_highlight;
        let splitter_col = if self.is_resizing {
            theme.caret
        } else {
            theme.selection_border
        };

        unsafe {
            let mem_dc = CreateCompatibleDC(Some(hdc));
            if mem_dc.is_invalid() {
                return;
            }
            let mem_bmp = CreateCompatibleBitmap(hdc, w, pane_height);
            if mem_bmp.is_invalid() {
                let _ = DeleteDC(mem_dc);
                return;
            }
            let old_bmp = SelectObject(mem_dc, mem_bmp.into());

            // Fill sidebar background
            let bg_brush = CreateSolidBrush(sidebar_bg.to_colorref());
            let full_rc = RECT {
                left: 0,
                top: 0,
                right: w,
                bottom: pane_height,
            };
            let _ = FillRect(mem_dc, &full_rc, bg_brush);
            let _ = DeleteObject(bg_brush.into());

            // Fill `"FOLDERS"` header background
            let hdr_brush = CreateSolidBrush(header_bg.to_colorref());
            let hdr_rc = RECT {
                left: 0,
                top: 0,
                right: w,
                bottom: SIDEBAR_HEADER_HEIGHT,
            };
            let _ = FillRect(mem_dc, &hdr_rc, hdr_brush);
            let _ = DeleteObject(hdr_brush.into());

            let font_name: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
            let hfont_bold = CreateFontW(
                -13,
                0,
                0,
                0,
                700,
                0,
                0,
                0,
                FONT_CHARSET(0),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                FONT_QUALITY(5),
                0,
                windows::core::PCWSTR(font_name.as_ptr()),
            );
            let hfont_row = CreateFontW(
                -15,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                FONT_CHARSET(0),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                FONT_QUALITY(5),
                0,
                windows::core::PCWSTR(font_name.as_ptr()),
            );

            let _ = SetBkMode(mem_dc, TRANSPARENT);

            // Draw `"FOLDERS"` heading (`sidebar_heading` in `Default.sublime-theme`)
            let old_font = SelectObject(mem_dc, hfont_bold.into());
            let _ = SetTextColor(mem_dc, theme.gutter_foreground.to_colorref());
            let mut hdr_text: Vec<u16> = "FOLDERS".encode_utf16().collect();
            let mut hdr_text_rc = RECT {
                left: 12,
                top: 0,
                right: w - 8,
                bottom: SIDEBAR_HEADER_HEIGHT,
            };
            let _ = DrawTextW(
                mem_dc,
                &mut hdr_text,
                &mut hdr_text_rc,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );

            // Draw visible tree rows
            let _ = SelectObject(mem_dc, hfont_row.into());
            let mut y = SIDEBAR_HEADER_HEIGHT;
            for (idx, row) in self.flat_rows.iter().enumerate().skip(self.scroll_row) {
                if y >= pane_height {
                    break;
                }
                let row_bottom = (y + SIDEBAR_ROW_HEIGHT).min(pane_height);
                let is_selected = !row.is_empty_placeholder
                    && self
                        .selected_path
                        .as_ref()
                        .map(|s| s == &row.path)
                        .unwrap_or(false);
                let is_hovered = self.hover_row == Some(idx);

                if is_selected || is_hovered {
                    let row_col = if is_selected { selected_bg } else { hover_bg };
                    let row_brush = CreateSolidBrush(row_col.to_colorref());
                    let row_rc = RECT {
                        left: 0,
                        top: y,
                        right: w - 2,
                        bottom: row_bottom,
                    };
                    let _ = FillRect(mem_dc, &row_rc, row_brush);
                    let _ = DeleteObject(row_brush.into());
                }

                // Indent (`12px` per depth level per `Default.sublime-theme:211`)
                let indent_px = 10 + (row.depth as i32 * 14);
                let label = if row.is_empty_placeholder {
                    row.name.clone()
                } else if row.is_dir {
                    let chevron = if row.is_expanded { "▾" } else { "▸" };
                    format!("{} [Dir] {}", chevron, row.name)
                } else {
                    format!("    {}", row.name)
                };

                let text_col = if row.is_empty_placeholder {
                    theme.gutter_foreground
                } else if is_selected {
                    theme.foreground
                } else if row.is_dir {
                    theme.foreground
                } else {
                    theme.foreground.blend(theme.gutter_foreground, 20, 100)
                };
                let _ = SetTextColor(mem_dc, text_col.to_colorref());

                let mut wide_row: Vec<u16> = label.encode_utf16().collect();
                let mut text_rc = RECT {
                    left: indent_px,
                    top: y,
                    right: (w - 6).max(indent_px + 10),
                    bottom: row_bottom,
                };
                let _ = DrawTextW(
                    mem_dc,
                    &mut wide_row,
                    &mut text_rc,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
                );

                y += SIDEBAR_ROW_HEIGHT;
            }

            // Draw right-edge resizable splitter line
            let pen = CreatePen(PS_SOLID, 2, splitter_col.to_colorref());
            let old_pen = SelectObject(mem_dc, pen.into());
            let _ = MoveToEx(mem_dc, w - 1, 0, None);
            let _ = LineTo(mem_dc, w - 1, pane_height);
            let _ = SelectObject(mem_dc, old_pen);
            let _ = DeleteObject(pen.into());

            let _ = SelectObject(mem_dc, old_font);
            let _ = DeleteObject(hfont_bold.into());
            let _ = DeleteObject(hfont_row.into());

            let _ = BitBlt(hdc, 0, 0, w, pane_height, Some(mem_dc), 0, 0, SRCCOPY);

            let _ = SelectObject(mem_dc, old_bmp);
            let _ = DeleteObject(mem_bmp.into());
            let _ = DeleteDC(mem_dc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_open_folder_populates_tree_and_filters_excluded_dirs() {
        let temp_dir = std::env::temp_dir().join(format!(
            "notepad_explorer_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join("src")).expect("create src");
        fs::create_dir_all(temp_dir.join(".git")).expect("create .git");
        fs::create_dir_all(temp_dir.join("empty_sub")).expect("create empty_sub");
        fs::write(temp_dir.join("README.md"), "# Test").expect("write readme");
        fs::write(temp_dir.join("src").join("main.rs"), "fn main() {}").expect("write main.rs");

        let mut explorer = FileExplorer::new();
        assert_eq!(explorer.effective_width(), 0);

        explorer.open_folder(temp_dir.clone()).expect("open_folder");
        assert!(explorer.is_visible);
        assert_eq!(explorer.effective_width(), DEFAULT_SIDEBAR_WIDTH);

        // Root + "empty_sub" (collapsed) + "src" (collapsed) + "README.md" (".git" excluded!)
        let names: Vec<String> = explorer.flat_rows.iter().map(|r| r.name.clone()).collect();
        assert!(names.contains(&"src".to_string()));
        assert!(names.contains(&"empty_sub".to_string()));
        assert!(names.contains(&"README.md".to_string()));
        assert!(!names.contains(&".git".to_string()));

        // Expand "src" -> "main.rs" appears
        explorer.toggle_expand(&temp_dir.join("src"));
        let names_after: Vec<String> = explorer.flat_rows.iter().map(|r| r.name.clone()).collect();
        assert!(names_after.contains(&"main.rs".to_string()));

        // Expand "empty_sub" -> "(empty folder)" placeholder appears cleanly
        explorer.toggle_expand(&temp_dir.join("empty_sub"));
        let names_empty: Vec<String> = explorer.flat_rows.iter().map(|r| r.name.clone()).collect();
        assert!(names_empty.contains(&"(empty folder)".to_string()));

        explorer.close_folder();
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
