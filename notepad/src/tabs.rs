//! Multi-Tab Document Model (`Tab`, `TabManager`) and GDI Tab Bar Widget.
//!
//! # Reverse Engineering Reference (`C:\Program Files\Sublime Text\sublime_text.exe`)
//! - `new_file` (`0x14005AD24`, vtable `0x1405B9DE0`): creates untitled buffer in active group.
//! - `open_tabs_after_current` (`0x1406B4D84` at `0x1400534FF` & `0x140042C73`): inserts new tabs
//!   immediately after `active_index`.
//! - `tabset_control` (`0x1400E0F84..0x1400E1618`), `tab_control` & `tab_close_button`
//!   (`0x14018EC59..0x14018ECF0`), `new_tab_button` (`0x1400E159D`).
//! - Context menu commands (`Tab Context.sublime-menu`):
//!   - `close_by_index` (`0x14005B298`)
//!   - `close_others_by_index` (`0x14005B8E6`)
//!   - `close_to_right_by_index` (`0x14005B582`)
//!   - `close_unmodified` (`0x14005BA5E`)
//! - Switching commands:
//!   - `next_view_in_stack` / `next_view` (`0x14005BEF4` / `0x14005BD84`, `Ctrl+Tab` / `Ctrl+PageDown`)
//!   - `prev_view_in_stack` / `prev_view` (`0x14005BF0A` / `0x14005BE3C`, `Ctrl+Shift+Tab` / `Ctrl+PageUp`)

use crate::cursor::MultiCursorEditor;
use crate::file_io::TextEncoding;
use crate::theme::{RgbColor, Theme};
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen, CreateSolidBrush,
    DeleteDC, DeleteObject, DrawTextW, FillRect, LineTo, MoveToEx, SelectObject, SetBkMode,
    SetTextColor, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
    FONT_CHARSET, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION, FONT_QUALITY, HDC, PS_SOLID,
    TRANSPARENT,
};

/// Height of the Sublime-style tab bar in pixels (`Default.sublime-theme`: `tab_height: 32`).
pub const TAB_BAR_HEIGHT: i32 = 32;
/// Minimum pixel width of an individual tab.
pub const TAB_MIN_WIDTH: i32 = 90;
/// Maximum pixel width of an individual tab.
pub const TAB_MAX_WIDTH: i32 = 210;
/// Width of the `+` (`new_tab_button`) at the right end of the tab strip.
pub const NEW_TAB_BTN_WIDTH: i32 = 30;
/// Size of the `×` close button hit box inside each tab.
pub const TAB_CLOSE_SIZE: i32 = 18;

/// Represents a single open document tab (modeled after Sublime Text's `Sheet` / `View` + `TextBuffer`).
#[derive(Debug, Clone)]
pub struct Tab {
    /// Unique monotonic identifier for this tab.
    pub id: u64,
    /// Backing filesystem path (`None` for untitled buffers).
    pub path: Option<PathBuf>,
    /// Base title derived from filename or `"Untitled"`.
    pub title: String,
    /// Full UTF-8 text buffer content of this tab.
    pub content: String,
    /// File encoding associated with this tab.
    pub encoding: TextEncoding,
    /// Whether this tab has unsaved modifications (`*` / `•`).
    pub is_dirty: bool,
    /// Whether this tab is a transient single-click preview tab (`attributes: ["transient"]`).
    pub is_preview: bool,
    /// Saved selection range `(start_utf16, end_utf16)` when switching away from this tab.
    pub selection: (u32, u32),
    /// Saved top visible line index (`EM_GETFIRSTVISIBLELINE`) when switching away from this tab.
    pub scroll_line: i32,
    /// Per-tab multi-cursor and rectangular block-selection state.
    pub multi_cursor: MultiCursorEditor,
}

impl Tab {
    /// Creates a new untitled pinned tab with the given `id`.
    pub fn new_untitled(id: u64) -> Self {
        Self {
            id,
            path: None,
            title: String::from("Untitled"),
            content: String::new(),
            encoding: TextEncoding::Utf8,
            is_dirty: false,
            is_preview: false,
            selection: (0, 0),
            scroll_line: 0,
            multi_cursor: MultiCursorEditor::new(),
        }
    }

    /// Creates a tab backed by a filesystem `path`.
    pub fn from_file(
        id: u64,
        path: PathBuf,
        content: String,
        encoding: TextEncoding,
        is_preview: bool,
    ) -> Self {
        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("Untitled")
            .to_string();
        Self {
            id,
            path: Some(path),
            title,
            content,
            encoding,
            is_dirty: false,
            is_preview,
            selection: (0, 0),
            scroll_line: 0,
            multi_cursor: MultiCursorEditor::new(),
        }
    }

    /// Computes the display label for the tab header, including `*` when `is_dirty` is true
    /// and mirroring Sublime's `set_unsaved_view_name.py` first-line fallback for untitled buffers.
    pub fn display_label(&self) -> String {
        let base = if self.path.is_some() {
            self.title.clone()
        } else {
            let first_line = self
                .content
                .lines()
                .next()
                .map(str::trim)
                .unwrap_or("");
            if !first_line.is_empty() {
                let snippet: String = first_line.chars().take(24).collect();
                snippet
            } else {
                self.title.clone()
            }
        };

        if self.is_dirty {
            format!("{} *", base)
        } else {
            base
        }
    }
}

/// Geometry of a laid-out tab in the tab bar for rendering and mouse hit-testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Result of hit-testing a mouse point `(x, y)` inside the tab bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabHitTarget {
    /// Mouse is over the body of tab `index`.
    TabBody(usize),
    /// Mouse is over the `×` close button of tab `index`.
    TabClose(usize),
    /// Mouse is over the `+` (`new_tab_button`).
    NewTabButton,
    /// Mouse is over empty background inside the tab bar.
    EmptyBar,
}

/// Manages the ordered list of open `Tab`s, active tab index, MRU stack, and tab bar UI state.
#[derive(Debug, Clone)]
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
    pub context_menu_tab: Option<usize>,
}

impl Default for TabManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TabManager {
    /// Initializes a `TabManager` with a single untitled tab (`id = 1`).
    pub fn new() -> Self {
        let initial_tab = Tab::new_untitled(1);
        Self {
            tabs: vec![initial_tab],
            active_index: 0,
            mru_stack: vec![1],
            next_id: 2,
            hover_tab: None,
            hover_close: None,
            hover_new_btn: false,
            dragging_tab: None,
            drag_start_x: 0,
            context_menu_tab: None,
        }
    }

    /// Returns a shared reference to the currently active `Tab`.
    pub fn active_tab(&self) -> &Tab {
        let idx = self.active_index.min(self.tabs.len().saturating_sub(1));
        &self.tabs[idx]
    }

    /// Returns a mutable reference to the currently active `Tab`.
    pub fn active_tab_mut(&mut self) -> &mut Tab {
        let idx = self.active_index.min(self.tabs.len().saturating_sub(1));
        &mut self.tabs[idx]
    }

    /// Records `tab_id` at the top (back) of the MRU stack (`next_view_in_stack`).
    fn touch_mru(&mut self, tab_id: u64) {
        self.mru_stack.retain(|&id| id != tab_id);
        self.mru_stack.push(tab_id);
    }

    /// Creates a new untitled tab immediately after the active tab (`open_tabs_after_current = true`)
    /// and makes it the active tab. Returns the new `active_index`.
    pub fn new_tab(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        let tab = Tab::new_untitled(id);
        let insert_idx = if self.tabs.is_empty() {
            0
        } else {
            (self.active_index + 1).min(self.tabs.len())
        };
        self.tabs.insert(insert_idx, tab);
        self.active_index = insert_idx;
        self.touch_mru(id);
        insert_idx
    }

    /// Finds an existing tab matching `path` (case-insensitive on Windows).
    pub fn find_tab_by_path(&self, path: &Path) -> Option<usize> {
        let target = normalize_path_key(path);
        self.tabs.iter().position(|t| {
            t.path
                .as_deref()
                .map(|p| normalize_path_key(p) == target)
                .unwrap_or(false)
        })
    }

    /// Opens or focuses a file in the tab manager:
    /// - If `path` is already open in a tab, switches to that tab; if `!is_preview`, pins it (`is_preview = false`).
    /// - If `is_preview == true`, replaces any existing clean preview tab (`t.is_preview && !t.is_dirty`) in-place.
    /// - If the only tab is an untouched empty untitled tab and `!is_preview`, reuses that slot.
    /// - Otherwise inserts a new tab right after `active_index`.
    /// Returns `(active_index, was_already_open)`.
    pub fn open_file_in_tab(
        &mut self,
        path: PathBuf,
        content: String,
        encoding: TextEncoding,
        is_preview: bool,
    ) -> (usize, bool) {
        if let Some(existing_idx) = self.find_tab_by_path(&path) {
            if !is_preview {
                self.tabs[existing_idx].is_preview = false;
            }
            self.switch_to(existing_idx);
            return (existing_idx, true);
        }

        // If opening in preview mode, reuse an existing unmodified preview tab if one exists
        if is_preview {
            if let Some(preview_idx) = self.tabs.iter().position(|t| t.is_preview && !t.is_dirty) {
                let id = self.next_id;
                self.next_id += 1;
                self.tabs[preview_idx] = Tab::from_file(id, path, content, encoding, true);
                self.switch_to(preview_idx);
                return (preview_idx, false);
            }
        } else if self.tabs.len() == 1
            && self.tabs[0].path.is_none()
            && !self.tabs[0].is_dirty
            && self.tabs[0].content.is_empty()
            && !self.tabs[0].is_preview
        {
            // Reuse the initial blank untitled tab when opening a pinned file directly
            let id = self.tabs[0].id;
            self.tabs[0] = Tab::from_file(id, path, content, encoding, false);
            self.active_index = 0;
            self.touch_mru(id);
            return (0, false);
        }

        let id = self.next_id;
        self.next_id += 1;
        let tab = Tab::from_file(id, path, content, encoding, is_preview);
        let insert_idx = if self.tabs.is_empty() {
            0
        } else {
            (self.active_index + 1).min(self.tabs.len())
        };
        self.tabs.insert(insert_idx, tab);
        self.active_index = insert_idx;
        self.touch_mru(id);
        (insert_idx, false)
    }

    /// Promotes a preview tab at `index` to a pinned tab (`is_preview = false`).
    pub fn pin_tab(&mut self, index: usize) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.is_preview = false;
        }
    }

    /// Marks the active tab dirty or clean. Editing a preview tab automatically pins it
    /// (`is_preview = false`), matching Sublime Text's transient buffer promotion.
    pub fn set_active_dirty(&mut self, is_dirty: bool) {
        if let Some(tab) = self.tabs.get_mut(self.active_index) {
            tab.is_dirty = is_dirty;
            if is_dirty {
                tab.is_preview = false;
            }
        }
    }

    /// Switches to the tab at `index` if valid. Returns `true` if `active_index` changed.
    pub fn switch_to(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        let changed = self.active_index != index;
        self.active_index = index;
        let id = self.tabs[index].id;
        self.touch_mru(id);
        changed
    }

    /// Cycles to the next tab (`Ctrl+Tab` / `Ctrl+PageDown`), wrapping around to `0`.
    pub fn next_tab(&mut self) -> usize {
        if self.tabs.is_empty() {
            return 0;
        }
        let next = (self.active_index + 1) % self.tabs.len();
        self.switch_to(next);
        self.active_index
    }

    /// Cycles to the previous tab (`Ctrl+Shift+Tab` / `Ctrl+PageUp`), wrapping around to `len - 1`.
    pub fn prev_tab(&mut self) -> usize {
        if self.tabs.is_empty() {
            return 0;
        }
        let prev = (self.active_index + self.tabs.len() - 1) % self.tabs.len();
        self.switch_to(prev);
        self.active_index
    }

    /// Reorders a tab from `from_index` to `to_index` (drag-to-reorder) while keeping the
    /// dragged tab active if it was active.
    pub fn reorder_tab(&mut self, from_index: usize, to_index: usize) -> bool {
        if from_index >= self.tabs.len() || to_index >= self.tabs.len() || from_index == to_index {
            return false;
        }
        let active_id = self.active_tab().id;
        let tab = self.tabs.remove(from_index);
        self.tabs.insert(to_index, tab);
        if let Some(new_active) = self.tabs.iter().position(|t| t.id == active_id) {
            self.active_index = new_active;
        }
        true
    }

    /// Closes the tab at `index`. If the last remaining tab is closed, replaces it with a fresh
    /// untitled tab (`close_windows_when_empty = false` on Windows).
    pub fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        let removed = self.tabs.remove(index);
        self.mru_stack.retain(|&id| id != removed.id);

        if self.tabs.is_empty() {
            let id = self.next_id;
            self.next_id += 1;
            self.tabs.push(Tab::new_untitled(id));
            self.active_index = 0;
            self.mru_stack.push(id);
            return;
        }

        if self.active_index > index {
            self.active_index -= 1;
        } else if self.active_index >= self.tabs.len() {
            self.active_index = self.tabs.len() - 1;
        }
        let active_id = self.tabs[self.active_index].id;
        self.touch_mru(active_id);
    }

    /// Closes all tabs except `keep_index` (`close_others_by_index` at `0x14005B8E6`).
    pub fn close_others(&mut self, keep_index: usize) {
        if keep_index >= self.tabs.len() {
            return;
        }
        let kept = self.tabs[keep_index].clone();
        let kept_id = kept.id;
        self.tabs = vec![kept];
        self.active_index = 0;
        self.mru_stack = vec![kept_id];
    }

    /// Closes all tabs to the right of `from_index` (`close_to_right_by_index` at `0x14005B582`).
    pub fn close_to_right(&mut self, from_index: usize) {
        if from_index + 1 >= self.tabs.len() {
            return;
        }
        self.tabs.truncate(from_index + 1);
        let valid_ids: Vec<u64> = self.tabs.iter().map(|t| t.id).collect();
        self.mru_stack.retain(|id| valid_ids.contains(id));
        if self.active_index >= self.tabs.len() {
            self.active_index = self.tabs.len() - 1;
        }
    }

    /// Closes all unmodified (`!is_dirty`) tabs (`close_unmodified` at `0x14005BA5E`).
    pub fn close_unmodified(&mut self) {
        let active_id = self.active_tab().id;
        self.tabs.retain(|t| t.is_dirty);
        if self.tabs.is_empty() {
            let id = self.next_id;
            self.next_id += 1;
            self.tabs.push(Tab::new_untitled(id));
            self.active_index = 0;
            self.mru_stack = vec![id];
            return;
        }
        self.active_index = self
            .tabs
            .iter()
            .position(|t| t.id == active_id)
            .unwrap_or(0);
    }

    /// Computes the horizontal pixel layout of all tabs and the `+` button within `[bar_left..bar_right]`.
    pub fn compute_layout(&self, bar_left: i32, bar_right: i32) -> (Vec<TabRect>, RECT) {
        let count = self.tabs.len().max(1) as i32;
        let available_w = (bar_right - bar_left - NEW_TAB_BTN_WIDTH - 6).max(TAB_MIN_WIDTH);
        let raw_tab_w = available_w / count;
        let tab_w = raw_tab_w.clamp(TAB_MIN_WIDTH, TAB_MAX_WIDTH);

        let mut rects = Vec::with_capacity(self.tabs.len());
        let mut x = bar_left;
        for idx in 0..self.tabs.len() {
            let right = (x + tab_w).min(bar_right - NEW_TAB_BTN_WIDTH - 2);
            let close_right = right - 6;
            let close_left = (close_right - TAB_CLOSE_SIZE).max(x + 4);
            let close_top = (TAB_BAR_HEIGHT - TAB_CLOSE_SIZE) / 2;
            let close_bottom = close_top + TAB_CLOSE_SIZE;
            rects.push(TabRect {
                index: idx,
                left: x,
                top: 0,
                right,
                bottom: TAB_BAR_HEIGHT,
                close_left,
                close_top,
                close_right,
                close_bottom,
            });
            x = right;
        }

        let new_btn_left = x + 2;
        let new_btn_rect = RECT {
            left: new_btn_left,
            top: 3,
            right: (new_btn_left + NEW_TAB_BTN_WIDTH).min(bar_right),
            bottom: TAB_BAR_HEIGHT - 3,
        };
        (rects, new_btn_rect)
    }

    /// Hit-tests client coordinates `(x, y)` against the tab bar spanning `[bar_left..bar_right, 0..TAB_BAR_HEIGHT]`.
    pub fn hit_test(&self, x: i32, y: i32, bar_left: i32, bar_right: i32) -> Option<TabHitTarget> {
        if x < bar_left || x >= bar_right || y < 0 || y >= TAB_BAR_HEIGHT {
            return None;
        }
        let (tab_rects, new_btn) = self.compute_layout(bar_left, bar_right);
        for r in &tab_rects {
            if x >= r.left && x < r.right && y >= r.top && y < r.bottom {
                if x >= r.close_left
                    && x < r.close_right
                    && y >= r.close_top
                    && y < r.close_bottom
                {
                    return Some(TabHitTarget::TabClose(r.index));
                }
                return Some(TabHitTarget::TabBody(r.index));
            }
        }
        if x >= new_btn.left && x < new_btn.right && y >= new_btn.top && y < new_btn.bottom {
            return Some(TabHitTarget::NewTabButton);
        }
        Some(TabHitTarget::EmptyBar)
    }

    /// Renders the Sublime-style Tab Bar using double-buffered Win32 GDI onto `hdc`
    /// inside `[bar_left, 0, bar_right, TAB_BAR_HEIGHT]`.
    pub fn render_gdi(&self, hdc: HDC, bar_left: i32, bar_right: i32, theme: &Theme) {
        let bar_w = (bar_right - bar_left).max(0);
        if bar_w <= 0 {
            return;
        }

        let strip_bg = if theme.is_dark {
            theme.background.blend(RgbColor::new(0, 0, 0), 32, 100)
        } else {
            theme.background.blend(RgbColor::new(0, 0, 0), 12, 100)
        };
        let inactive_tab_bg = if theme.is_dark {
            theme.background.blend(RgbColor::new(0, 0, 0), 18, 100)
        } else {
            theme.background.blend(RgbColor::new(0, 0, 0), 6, 100)
        };
        let hover_tab_bg = if theme.is_dark {
            theme.background.blend(RgbColor::new(255, 255, 255), 6, 100)
        } else {
            theme.background.blend(RgbColor::new(0, 0, 0), 3, 100)
        };
        let active_tab_bg = theme.background;
        let border_col = theme.selection_border;
        let accent_col = theme.caret;

        unsafe {
            let mem_dc = CreateCompatibleDC(Some(hdc));
            if mem_dc.is_invalid() {
                return;
            }
            let mem_bmp = CreateCompatibleBitmap(hdc, bar_w, TAB_BAR_HEIGHT);
            if mem_bmp.is_invalid() {
                let _ = DeleteDC(mem_dc);
                return;
            }
            let old_bmp = SelectObject(mem_dc, mem_bmp.into());

            // Fill tab bar background
            let bg_brush = CreateSolidBrush(strip_bg.to_colorref());
            let full_rc = RECT {
                left: 0,
                top: 0,
                right: bar_w,
                bottom: TAB_BAR_HEIGHT,
            };
            let _ = FillRect(mem_dc, &full_rc, bg_brush);
            let _ = DeleteObject(bg_brush.into());

            // Create normal and italic UI fonts for regular vs preview tabs (`Default.sublime-theme:736`)
            let font_name: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
            let hfont_normal = CreateFontW(
                -15,
                0,
                0,
                0,
                500,
                0,
                0,
                0,
                FONT_CHARSET(0),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                FONT_QUALITY(5), // CLEARTYPE_QUALITY
                0,
                windows::core::PCWSTR(font_name.as_ptr()),
            );
            let hfont_italic = CreateFontW(
                -15,
                0,
                0,
                0,
                400,
                1, // Italic for preview tabs (`attributes: ["transient"]`)
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
            let (tab_rects, new_btn) = self.compute_layout(0, bar_w);

            let border_pen = CreatePen(PS_SOLID, 1, border_col.to_colorref());
            let old_pen = SelectObject(mem_dc, border_pen.into());

            // Bottom connector line across the whole tab bar
            let _ = MoveToEx(mem_dc, 0, TAB_BAR_HEIGHT - 1, None);
            let _ = LineTo(mem_dc, bar_w, TAB_BAR_HEIGHT - 1);

            for (tab, rect) in self.tabs.iter().zip(tab_rects.iter()) {
                let is_active = rect.index == self.active_index;
                let is_hovered = self.hover_tab == Some(rect.index);
                let fill_color = if is_active {
                    active_tab_bg
                } else if is_hovered {
                    hover_tab_bg
                } else {
                    inactive_tab_bg
                };

                let tab_rc = RECT {
                    left: rect.left,
                    top: rect.top + if is_active { 0 } else { 3 },
                    right: rect.right,
                    bottom: if is_active {
                        TAB_BAR_HEIGHT
                    } else {
                        TAB_BAR_HEIGHT - 1
                    },
                };
                let tab_brush = CreateSolidBrush(fill_color.to_colorref());
                let _ = FillRect(mem_dc, &tab_rc, tab_brush);
                let _ = DeleteObject(tab_brush.into());

                // Right divider between tabs
                let _ = MoveToEx(mem_dc, rect.right - 1, tab_rc.top + 4, None);
                let _ = LineTo(mem_dc, rect.right - 1, TAB_BAR_HEIGHT - 4);

                // Top accent bar on the active tab (`connector_height: 2` in Sublime theme)
                if is_active {
                    let accent_rc = RECT {
                        left: rect.left,
                        top: 0,
                        right: rect.right,
                        bottom: 3,
                    };
                    let accent_brush = CreateSolidBrush(accent_col.to_colorref());
                    let _ = FillRect(mem_dc, &accent_rc, accent_brush);
                    let _ = DeleteObject(accent_brush.into());
                }

                // Select italic font for preview tabs, normal font for pinned tabs
                let old_font = if tab.is_preview {
                    SelectObject(mem_dc, hfont_italic.into())
                } else {
                    SelectObject(mem_dc, hfont_normal.into())
                };

                let text_col = if is_active {
                    theme.foreground
                } else if tab.is_dirty {
                    accent_col
                } else {
                    theme.gutter_foreground_highlight.blend(theme.gutter_foreground, 50, 100)
                };
                let _ = SetTextColor(mem_dc, text_col.to_colorref());

                let label = tab.display_label();
                let mut wide_label: Vec<u16> = label.encode_utf16().collect();
                let mut text_rc = RECT {
                    left: rect.left + 10,
                    top: rect.top + 2,
                    right: (rect.close_left - 4).max(rect.left + 12),
                    bottom: rect.bottom,
                };
                let _ = DrawTextW(
                    mem_dc,
                    &mut wide_label,
                    &mut text_rc,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
                );

                // Draw close button (`×`) or dirty indicator (`•`) matching `Default.sublime-theme:752-776`
                let is_close_hovered = self.hover_close == Some(rect.index);
                if is_close_hovered {
                    let close_bg = CreateSolidBrush(theme.selection.to_colorref());
                    let close_rc = RECT {
                        left: rect.close_left,
                        top: rect.close_top,
                        right: rect.close_right,
                        bottom: rect.close_bottom,
                    };
                    let _ = FillRect(mem_dc, &close_rc, close_bg);
                    let _ = DeleteObject(close_bg.into());
                }

                let _ = SelectObject(mem_dc, hfont_normal.into());
                let glyph = if tab.is_dirty && !is_close_hovered {
                    "•"
                } else {
                    "×"
                };
                let glyph_col = if is_close_hovered {
                    theme.foreground
                } else if tab.is_dirty {
                    accent_col
                } else {
                    theme.gutter_foreground
                };
                let _ = SetTextColor(mem_dc, glyph_col.to_colorref());
                let mut wide_glyph: Vec<u16> = glyph.encode_utf16().collect();
                let mut close_rc = RECT {
                    left: rect.close_left,
                    top: rect.close_top,
                    right: rect.close_right,
                    bottom: rect.close_bottom,
                };
                let _ = DrawTextW(
                    mem_dc,
                    &mut wide_glyph,
                    &mut close_rc,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );

                let _ = SelectObject(mem_dc, old_font);
            }

            // Draw `+` (`new_tab_button`)
            if self.hover_new_btn {
                let btn_brush = CreateSolidBrush(hover_tab_bg.to_colorref());
                let _ = FillRect(mem_dc, &new_btn, btn_brush);
                let _ = DeleteObject(btn_brush.into());
            }
            let old_font = SelectObject(mem_dc, hfont_normal.into());
            let _ = SetTextColor(mem_dc, theme.foreground.to_colorref());
            let mut plus_wide: Vec<u16> = "+".encode_utf16().collect();
            let mut plus_rc = new_btn;
            let _ = DrawTextW(
                mem_dc,
                &mut plus_wide,
                &mut plus_rc,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
            let _ = SelectObject(mem_dc, old_font);

            let _ = SelectObject(mem_dc, old_pen);
            let _ = DeleteObject(border_pen.into());
            let _ = DeleteObject(hfont_normal.into());
            let _ = DeleteObject(hfont_italic.into());

            let _ = windows::Win32::Graphics::Gdi::BitBlt(
                hdc,
                bar_left,
                0,
                bar_w,
                TAB_BAR_HEIGHT,
                Some(mem_dc),
                0,
                0,
                windows::Win32::Graphics::Gdi::SRCCOPY,
            );

            let _ = SelectObject(mem_dc, old_bmp);
            let _ = DeleteObject(mem_bmp.into());
            let _ = DeleteDC(mem_dc);
        }
    }
}

fn normalize_path_key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tab_inserts_after_active_and_switches() {
        let mut mgr = TabManager::new();
        assert_eq!(mgr.tabs.len(), 1);
        assert_eq!(mgr.active_index, 0);

        let idx1 = mgr.new_tab();
        assert_eq!(idx1, 1);
        assert_eq!(mgr.tabs.len(), 2);
        assert_eq!(mgr.active_index, 1);

        mgr.switch_to(0);
        let idx2 = mgr.new_tab();
        assert_eq!(idx2, 1);
        assert_eq!(mgr.tabs.len(), 3);
        assert_eq!(mgr.active_index, 1);
    }

    #[test]
    fn test_preview_tab_replaces_clean_preview_until_pinned_or_dirty() {
        let mut mgr = TabManager::new();
        // Make tab 0 dirty so it isn't reused as blank untitled
        mgr.set_active_dirty(true);

        // Single-click file A -> opens as preview tab at index 1
        let (idx_a, existed_a) = mgr.open_file_in_tab(
            PathBuf::from(r"D:\test\a.rs"),
            "fn a() {}".to_string(),
            TextEncoding::Utf8,
            true,
        );
        assert!(!existed_a);
        assert_eq!(idx_a, 1);
        assert_eq!(mgr.tabs.len(), 2);
        assert!(mgr.tabs[1].is_preview);

        // Single-click file B -> replaces preview tab 1 in-place!
        let (idx_b, existed_b) = mgr.open_file_in_tab(
            PathBuf::from(r"D:\test\b.rs"),
            "fn b() {}".to_string(),
            TextEncoding::Utf8,
            true,
        );
        assert!(!existed_b);
        assert_eq!(idx_b, 1);
        assert_eq!(mgr.tabs.len(), 2);
        assert_eq!(mgr.tabs[1].title, "b.rs");
        assert!(mgr.tabs[1].is_preview);

        // Double-click file B -> pins tab 1
        let (idx_b_pin, existed_b_pin) = mgr.open_file_in_tab(
            PathBuf::from(r"D:\test\b.rs"),
            "fn b() {}".to_string(),
            TextEncoding::Utf8,
            false,
        );
        assert!(existed_b_pin);
        assert_eq!(idx_b_pin, 1);
        assert!(!mgr.tabs[1].is_preview);

        // Now single-click file C -> opens in a new preview tab 2 without replacing pinned B
        let (idx_c, _) = mgr.open_file_in_tab(
            PathBuf::from(r"D:\test\c.rs"),
            "fn c() {}".to_string(),
            TextEncoding::Utf8,
            true,
        );
        assert_eq!(idx_c, 2);
        assert_eq!(mgr.tabs.len(), 3);
        assert!(mgr.tabs[2].is_preview);

        // Editing preview tab C auto-pins it and shows '*'
        mgr.set_active_dirty(true);
        assert!(!mgr.tabs[2].is_preview);
        assert!(mgr.tabs[2].is_dirty);
        assert_eq!(mgr.tabs[2].display_label(), "c.rs *");
    }

    #[test]
    fn test_tab_reorder_close_others_and_close_to_right() {
        let mut mgr = TabManager::new();
        mgr.tabs[0].title = "One".to_string();
        mgr.new_tab();
        mgr.tabs[1].title = "Two".to_string();
        mgr.new_tab();
        mgr.tabs[2].title = "Three".to_string();
        mgr.new_tab();
        mgr.tabs[3].title = "Four".to_string();

        // Drag "Four" (index 3) to index 1
        assert!(mgr.reorder_tab(3, 1));
        assert_eq!(mgr.tabs[1].title, "Four");
        assert_eq!(mgr.active_index, 1);

        // Close tabs to the right of index 1 ("Two", "Three")
        mgr.close_to_right(1);
        assert_eq!(mgr.tabs.len(), 2);
        assert_eq!(mgr.tabs[0].title, "One");
        assert_eq!(mgr.tabs[1].title, "Four");

        // Close others keeping index 1 ("Four")
        mgr.close_others(1);
        assert_eq!(mgr.tabs.len(), 1);
        assert_eq!(mgr.tabs[0].title, "Four");
        assert_eq!(mgr.active_index, 0);
    }

    #[test]
    fn test_ctrl_tab_cyclic_switching() {
        let mut mgr = TabManager::new();
        mgr.new_tab();
        mgr.new_tab();
        assert_eq!(mgr.tabs.len(), 3);
        mgr.switch_to(0);
        assert_eq!(mgr.next_tab(), 1);
        assert_eq!(mgr.next_tab(), 2);
        assert_eq!(mgr.next_tab(), 0);
        assert_eq!(mgr.prev_tab(), 2);
    }
}
