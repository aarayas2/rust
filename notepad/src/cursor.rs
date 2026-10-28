//! Multi-Cursor and Block Selection (Column Selection) State & Editing Engine.
//!
//! Modeled after the reference architecture in `fresh-editor-master`:
//! - `crates/fresh-editor-core/src/model/cursor.rs` (`CursorId`, `SelectionMode`, `Position2D`, `Cursor`, `Cursors`)
//! - `crates/fresh-editor/src/input/multi_cursor.rs` (`add_cursor_above`, `add_cursor_below`, `add_cursor_at_next_match`, `line_end_positions_in_selection`)
//! - `crates/fresh-editor/src/input/actions.rs` (`block_select_action`, `convert_block_selection_to_cursors`, reverse-sorted multi-cursor bulk edits)
//! - `crates/fresh-editor/src/view/ui/split_rendering/orchestration/selection_sweep.rs` (`SelectionContext`, `SelectionActiveSet`)

use std::cmp::Reverse;
use std::ops::Range;

/// Unique identifier for a cursor in the editor (`CursorId` in `fresh-editor-core`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CursorId(pub usize);

impl CursorId {
    /// Sentinel value representing an invalid or unassigned cursor ID.
    pub const INVALID: Self = Self(usize::MAX);
}

/// Selection mode for a cursor (`SelectionMode` in `fresh-editor-core`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionMode {
    /// Standard stream selection (linear byte range across lines).
    #[default]
    Normal,
    /// Rectangular/columnar block selection anchored in 2D `(line, column)` space.
    Block,
}

/// 2D coordinates `(line, column)` (0-indexed) used for rectangular block selections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position2D {
    pub line: usize,
    pub column: usize,
}

/// Direction for keyboard-driven rectangular block selection (`Shift+Alt+Arrows`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Direction for single/multi-cursor movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDirection {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

/// A single cursor with its UTF-8 byte position, optional selection anchor,
/// sticky column for vertical navigation, and rectangular block selection state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    /// Current caret byte offset in the UTF-8 document buffer.
    pub position: usize,
    /// Optional byte offset where the selection started (`None` when collapsed).
    pub anchor: Option<usize>,
    /// Remembered character column for `Up`/`Down` navigation across short lines.
    pub sticky_column: Option<usize>,
    /// Selection mode (`Normal` stream selection or `Block` rectangular selection).
    pub selection_mode: SelectionMode,
    /// 2D anchor position `(line, column)` when `selection_mode == SelectionMode::Block`.
    pub block_anchor: Option<Position2D>,
    /// If true, an unshifted Left/Right arrow collapses a selection to its start/end edge.
    pub deselect_on_move: bool,
}

impl Cursor {
    /// Creates a new collapsed cursor at the given byte position.
    pub fn new(position: usize) -> Self {
        Self {
            position,
            anchor: None,
            sticky_column: None,
            selection_mode: SelectionMode::Normal,
            block_anchor: None,
            deselect_on_move: true,
        }
    }

    /// Creates a new cursor with a linear stream selection from `anchor` to `position`.
    pub fn with_selection(anchor: usize, position: usize) -> Self {
        Self {
            position,
            anchor: if anchor == position {
                None
            } else {
                Some(anchor)
            },
            sticky_column: None,
            selection_mode: SelectionMode::Normal,
            block_anchor: None,
            deselect_on_move: true,
        }
    }

    /// Creates a new cursor in rectangular `Block` selection mode.
    pub fn new_block(position: usize, anchor_2d: Position2D) -> Self {
        Self {
            position,
            anchor: None,
            sticky_column: Some(anchor_2d.column),
            selection_mode: SelectionMode::Block,
            block_anchor: Some(anchor_2d),
            deselect_on_move: true,
        }
    }

    /// Returns `true` if the cursor has no active selection.
    #[inline]
    pub fn collapsed(&self) -> bool {
        match self.selection_mode {
            SelectionMode::Normal => self.anchor.is_none_or(|a| a == self.position),
            SelectionMode::Block => self.block_anchor.is_none(),
        }
    }

    /// Returns the ordered byte range `start..end` of a normal selection, or `None` if collapsed.
    #[inline]
    pub fn selection_range(&self) -> Option<Range<usize>> {
        self.anchor.and_then(|anchor| {
            if anchor < self.position {
                Some(anchor..self.position)
            } else if anchor > self.position {
                Some(self.position..anchor)
            } else {
                None
            }
        })
    }

    /// Starts or resets a rectangular block selection at the given 2D coordinates.
    pub fn start_block_selection(&mut self, line: usize, column: usize) {
        self.selection_mode = SelectionMode::Block;
        self.block_anchor = Some(Position2D { line, column });
    }

    /// Clears rectangular block selection mode and returns to normal selection mode.
    pub fn clear_block_selection(&mut self) {
        self.selection_mode = SelectionMode::Normal;
        self.block_anchor = None;
    }

    /// Returns `true` if this cursor has an active rectangular block selection.
    #[inline]
    pub fn has_block_selection(&self) -> bool {
        self.selection_mode == SelectionMode::Block && self.block_anchor.is_some()
    }

    /// Clears any active stream or block selection while keeping `position`.
    pub fn clear_selection(&mut self) {
        self.anchor = None;
        self.clear_block_selection();
    }

    /// Sets the selection anchor if not already set.
    pub fn set_anchor(&mut self, anchor: usize) {
        if self.anchor.is_none() {
            self.anchor = Some(anchor);
        }
    }

    /// Moves the cursor to `position`, optionally extending the current selection.
    pub fn move_to(&mut self, position: usize, extend_selection: bool) {
        if extend_selection {
            if self.anchor.is_none() {
                self.anchor = Some(self.position);
            }
            self.position = position;
        } else {
            self.anchor = None;
            self.clear_block_selection();
            self.position = position;
        }
    }

    /// Returns the start byte offset of the selection (or `position` if collapsed).
    #[inline]
    pub fn selection_start(&self) -> usize {
        self.anchor
            .map(|a| a.min(self.position))
            .unwrap_or(self.position)
    }

    /// Returns the end byte offset of the selection (or `position` if collapsed).
    #[inline]
    pub fn selection_end(&self) -> usize {
        self.anchor
            .map(|a| a.max(self.position))
            .unwrap_or(self.position)
    }

    /// Adjusts `position` and `anchor` after a buffer edit at `edit_pos` replacing `old_len` bytes with `new_len` bytes.
    pub fn adjust_for_edit(&mut self, edit_pos: usize, old_len: usize, new_len: usize) {
        self.position = Self::adjust_offset(self.position, edit_pos, old_len, new_len);
        if let Some(anchor) = self.anchor {
            self.anchor = Some(Self::adjust_offset(anchor, edit_pos, old_len, new_len));
        }
    }

    fn adjust_offset(pos: usize, edit_pos: usize, old_len: usize, new_len: usize) -> usize {
        if pos <= edit_pos {
            pos
        } else if pos >= edit_pos + old_len {
            pos - old_len + new_len
        } else {
            edit_pos + new_len
        }
    }
}

/// Manages a collection of cursors (always at least one primary cursor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursors {
    cursors: Vec<(CursorId, Cursor)>,
    next_id: usize,
    primary_id: CursorId,
}

impl Default for Cursors {
    fn default() -> Self {
        Self::new()
    }
}

impl Cursors {
    /// Creates a new `Cursors` collection with a single primary cursor at byte offset 0.
    pub fn new() -> Self {
        let primary_id = CursorId(0);
        Self {
            cursors: vec![(primary_id, Cursor::new(0))],
            next_id: 1,
            primary_id,
        }
    }

    /// Creates a `Cursors` collection from a single `Cursor`.
    pub fn from_single(cursor: Cursor) -> Self {
        let primary_id = CursorId(0);
        Self {
            cursors: vec![(primary_id, cursor)],
            next_id: 1,
            primary_id,
        }
    }

    /// Returns a reference to the primary cursor.
    pub fn primary(&self) -> &Cursor {
        self.cursors
            .iter()
            .find(|(id, _)| *id == self.primary_id)
            .map(|(_, c)| c)
            .unwrap_or(&self.cursors[0].1)
    }

    /// Returns a mutable reference to the primary cursor.
    pub fn primary_mut(&mut self) -> &mut Cursor {
        let primary_id = self.primary_id;
        if let Some(idx) = self.cursors.iter().position(|(id, _)| *id == primary_id) {
            &mut self.cursors[idx].1
        } else {
            &mut self.cursors[0].1
        }
    }

    /// Returns the `CursorId` of the primary cursor.
    #[inline]
    pub fn primary_id(&self) -> CursorId {
        self.primary_id
    }

    /// Sets the primary cursor to `id` if it exists.
    pub fn set_primary(&mut self, id: CursorId) -> bool {
        if self.cursors.iter().any(|(cid, _)| *cid == id) {
            self.primary_id = id;
            true
        } else {
            false
        }
    }

    /// Gets a cursor by `CursorId`.
    pub fn get(&self, id: CursorId) -> Option<&Cursor> {
        self.cursors
            .iter()
            .find(|(cid, _)| *cid == id)
            .map(|(_, c)| c)
    }

    /// Gets a mutable cursor by `CursorId`.
    pub fn get_mut(&mut self, id: CursorId) -> Option<&mut Cursor> {
        self.cursors
            .iter_mut()
            .find(|(cid, _)| *cid == id)
            .map(|(_, c)| c)
    }

    /// Adds a new cursor and normalizes (sorts and merges duplicates).
    pub fn add(&mut self, cursor: Cursor) -> CursorId {
        let id = CursorId(self.next_id);
        self.next_id += 1;
        self.cursors.push((id, cursor));
        self.normalize();
        id
    }

    /// Adds a cursor without normalizing (used during batch operations followed by a single `normalize()`).
    pub fn add_direct(&mut self, cursor: Cursor) -> CursorId {
        let id = CursorId(self.next_id);
        self.next_id += 1;
        self.cursors.push((id, cursor));
        id
    }

    /// Toggles a cursor at `position` (`Alt+Click` behavior):
    /// - If multiple cursors exist and one is already at `position`, removes it.
    /// - Otherwise adds a new cursor at `position` and marks it primary.
    pub fn toggle_at(&mut self, position: usize) -> CursorId {
        if self.cursors.len() > 1 {
            if let Some(idx) = self
                .cursors
                .iter()
                .position(|(_, c)| c.position == position && c.collapsed())
            {
                let removed_id = self.cursors[idx].0;
                self.cursors.remove(idx);
                if self.primary_id == removed_id {
                    self.primary_id = self.cursors[0].0;
                }
                return self.primary_id;
            }
        }
        let id = self.add(Cursor::new(position));
        if self.cursors.iter().any(|(cid, _)| *cid == id) {
            self.primary_id = id;
        }
        self.primary_id
    }

    /// Removes a cursor by `CursorId` (will not remove the last remaining cursor).
    pub fn remove(&mut self, id: CursorId) -> Option<Cursor> {
        if self.cursors.len() <= 1 {
            return None;
        }
        if let Some(idx) = self.cursors.iter().position(|(cid, _)| *cid == id) {
            let (_, removed) = self.cursors.remove(idx);
            if self.primary_id == id {
                self.primary_id = self.cursors[0].0;
            }
            Some(removed)
        } else {
            None
        }
    }

    /// Removes all secondary cursors, keeping only the original (lowest-ID) cursor as primary.
    pub fn remove_secondary(&mut self) {
        if let Some(&(min_id, _)) = self.cursors.iter().min_by_key(|(id, _)| id.0) {
            self.primary_id = min_id;
        }
        let primary_id = self.primary_id;
        self.cursors.retain(|(id, _)| *id == primary_id);
        if let Some((_, c)) = self.cursors.first_mut() {
            c.clear_block_selection();
        }
    }

    /// Returns the total number of active cursors.
    #[inline]
    pub fn count(&self) -> usize {
        self.cursors.len()
    }

    /// Returns `true` if more than one cursor is active.
    #[inline]
    pub fn is_multi(&self) -> bool {
        self.cursors.len() > 1
    }

    /// Returns `true` if any cursor has a non-empty stream or block selection.
    pub fn has_any_selection(&self) -> bool {
        self.cursors
            .iter()
            .any(|(_, c)| !c.collapsed() || c.has_block_selection())
    }

    /// Returns `true` if any cursor has an active rectangular block selection.
    pub fn has_block_selection(&self) -> bool {
        self.cursors.iter().any(|(_, c)| c.has_block_selection())
    }

    /// Iterates over all `(CursorId, &Cursor)` pairs in document order.
    pub fn iter(&self) -> impl Iterator<Item = (CursorId, &Cursor)> {
        self.cursors.iter().map(|(id, c)| (*id, c))
    }

    /// Iterates mutably over all `(CursorId, &mut Cursor)` pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (CursorId, &mut Cursor)> {
        self.cursors.iter_mut().map(|(id, c)| (*id, c))
    }

    /// Sorts cursors by document position and deduplicates cursors with identical `(position, anchor)`
    /// or overlapping selections, preserving `primary_id` validity.
    pub fn normalize(&mut self) {
        self.cursors
            .sort_by_key(|(id, c)| (c.selection_start(), c.selection_end(), id.0));

        let mut deduped: Vec<(CursorId, Cursor)> = Vec::with_capacity(self.cursors.len());
        for (id, cursor) in self.cursors.drain(..) {
            if let Some((prev_id, prev_cursor)) = deduped.last_mut() {
                let same_pos = prev_cursor.position == cursor.position
                    && prev_cursor.anchor == cursor.anchor;
                let overlapping_ranges = match (prev_cursor.selection_range(), cursor.selection_range()) {
                    (Some(r1), Some(r2)) => r1.start < r2.end && r2.start < r1.end,
                    _ => false,
                };

                if same_pos || overlapping_ranges {
                    if overlapping_ranges {
                        let merged_start = prev_cursor.selection_start().min(cursor.selection_start());
                        let merged_end = prev_cursor.selection_end().max(cursor.selection_end());
                        prev_cursor.anchor = Some(merged_start);
                        prev_cursor.position = merged_end;
                    }
                    if id == self.primary_id {
                        *prev_id = id;
                    }
                    continue;
                }
            }
            deduped.push((id, cursor));
        }

        if deduped.is_empty() {
            let id = CursorId(0);
            deduped.push((id, Cursor::new(0)));
            self.primary_id = id;
        } else if !deduped.iter().any(|(id, _)| *id == self.primary_id) {
            self.primary_id = deduped[0].0;
        }

        self.cursors = deduped;
    }

    /// Adjusts all cursors for an edit at `edit_pos` replacing `old_len` bytes with `new_len` bytes.
    pub fn adjust_for_edit(&mut self, edit_pos: usize, old_len: usize, new_len: usize) {
        for (_, cursor) in &mut self.cursors {
            cursor.adjust_for_edit(edit_pos, old_len, new_len);
        }
        self.normalize();
    }
}

/// Precomputed selection and caret geometry used by the rendering pipeline (`SelectionContext` in SRC).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionContext {
    /// Normal linear selection byte ranges, sorted by `start`.
    pub ranges: Vec<Range<usize>>,
    /// Rectangular block selections as `(start_line, start_col, end_line, end_col)`.
    /// Lines are closed (`start_line..=end_line`), columns are half-open (`start_col..end_col`).
    pub block_rects: Vec<(usize, usize, usize, usize)>,
    /// All active cursor byte positions in document order.
    pub cursor_positions: Vec<usize>,
    /// Byte position of the primary cursor.
    pub primary_cursor_position: usize,
}

/// Sweep-line active selection tracker across lines and columns (`SelectionActiveSet` in SRC).
pub struct SelectionActiveSet<'a> {
    ranges: &'a [Range<usize>],
    range_idx: usize,
    block_rects: &'a [(usize, usize, usize, usize)],
    active_blocks: Vec<(usize, usize)>,
    last_line: Option<usize>,
}

impl<'a> SelectionActiveSet<'a> {
    /// Creates a new sweep-line selection tracker over a `SelectionContext`.
    pub fn new(ctx: &'a SelectionContext) -> Self {
        Self {
            ranges: &ctx.ranges,
            range_idx: 0,
            block_rects: &ctx.block_rects,
            active_blocks: Vec::new(),
            last_line: None,
        }
    }

    /// Updates active rectangular block column spans when entering `line` (0-indexed).
    pub fn enter_line(&mut self, line: usize) {
        if self.last_line == Some(line) {
            return;
        }
        self.last_line = Some(line);
        self.active_blocks.clear();
        for &(start_line, start_col, end_line, end_col) in self.block_rects {
            if line >= start_line && line <= end_line && start_col < end_col {
                self.active_blocks.push((start_col, end_col));
            }
        }
    }

    /// Returns `true` if the character at `(byte_pos, line_column)` is inside any normal or block selection.
    #[inline]
    pub fn contains(&mut self, byte_pos: usize, line_column: usize) -> bool {
        while self.range_idx < self.ranges.len() && self.ranges[self.range_idx].end <= byte_pos {
            self.range_idx += 1;
        }
        if self.range_idx < self.ranges.len() && self.ranges[self.range_idx].start <= byte_pos {
            return true;
        }
        self.active_blocks
            .iter()
            .any(|&(sc, ec)| line_column >= sc && line_column < ec)
    }
}

/// Converts a UTF-8 byte offset in `text` to a UTF-16 code unit offset (for Win32 Edit messages).
pub fn byte_to_utf16_offset(text: &str, byte_offset: usize) -> usize {
    let clamped = byte_offset.min(text.len());
    let valid_end = floor_char_boundary(text, clamped);
    text[..valid_end].encode_utf16().count()
}

/// Converts a UTF-16 code unit offset (from Win32 Edit messages) to a UTF-8 byte offset in `text`.
pub fn utf16_to_byte_offset(text: &str, utf16_offset: usize) -> usize {
    let mut u16_count = 0;
    for (byte_idx, ch) in text.char_indices() {
        if u16_count >= utf16_offset {
            return byte_idx;
        }
        u16_count += ch.len_utf16();
    }
    text.len()
}

fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// High-level multi-cursor and block selection text buffer manager.
#[derive(Debug, Clone)]
pub struct MultiCursorEditor {
    text: String,
    pub cursors: Cursors,
    undo_stack: Vec<(String, Cursors)>,
}

impl Default for MultiCursorEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl MultiCursorEditor {
    /// Creates an empty `MultiCursorEditor` with a single primary cursor at offset 0.
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursors: Cursors::new(),
            undo_stack: Vec::new(),
        }
    }

    /// Creates a `MultiCursorEditor` initialized with `text`.
    pub fn with_text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            cursors: Cursors::new(),
            undo_stack: Vec::new(),
        }
    }

    /// Returns a reference to the current UTF-8 document text.
    #[inline]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the entire buffer text and resets cursors to offset 0.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursors = Cursors::new();
        self.undo_stack.clear();
    }

    /// Synchronizes the buffer text and primary cursor selection from external single-cursor state
    /// (such as the Win32 Edit control when operating in single-cursor mode).
    pub fn sync_from_single_selection(
        &mut self,
        new_text: String,
        sel_start_utf16: usize,
        sel_end_utf16: usize,
    ) {
        let text_changed = self.text != new_text;
        if text_changed {
            self.text = new_text;
        }
        if !self.cursors.is_multi() && !self.cursors.has_block_selection() {
            let start_byte = utf16_to_byte_offset(&self.text, sel_start_utf16);
            let end_byte = utf16_to_byte_offset(&self.text, sel_end_utf16);
            let primary = self.cursors.primary_mut();
            if start_byte == end_byte {
                primary.position = end_byte;
                primary.anchor = None;
            } else {
                primary.anchor = Some(start_byte);
                primary.position = end_byte;
            }
        }
    }

    /// Pushes the current `(text, cursors)` snapshot onto the undo stack.
    fn save_undo_snapshot(&mut self) {
        const MAX_UNDO_STEPS: usize = 128;
        if self.undo_stack.len() >= MAX_UNDO_STEPS {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push((self.text.clone(), self.cursors.clone()));
    }

    /// Restores the most recent undo snapshot if available.
    pub fn undo(&mut self) -> bool {
        if let Some((prev_text, prev_cursors)) = self.undo_stack.pop() {
            self.text = prev_text;
            self.cursors = prev_cursors;
            true
        } else {
            false
        }
    }

    /// Returns `true` if an undo snapshot is available.
    #[inline]
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Returns the number of lines in the document (always at least 1).
    pub fn line_count(&self) -> usize {
        self.text.bytes().filter(|&b| b == b'\n').count() + 1
    }

    /// Returns a list of byte ranges `start..end` for every line in the buffer,
    /// where `end` excludes trailing `\r\n` or `\n`.
    pub fn line_content_ranges(&self) -> Vec<Range<usize>> {
        let mut ranges = Vec::new();
        let bytes = self.text.as_bytes();
        let mut line_start = 0;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\n' {
                let content_end = if i > line_start && bytes[i - 1] == b'\r' {
                    i - 1
                } else {
                    i
                };
                ranges.push(line_start..content_end);
                line_start = i + 1;
            }
            i += 1;
        }
        ranges.push(line_start..bytes.len());
        ranges
    }

    /// Returns the content byte range (excluding `\r\n` / `\n`) for the 0-indexed `line_idx`.
    pub fn line_range_bytes(&self, line_idx: usize) -> Range<usize> {
        let ranges = self.line_content_ranges();
        if line_idx < ranges.len() {
            ranges[line_idx].clone()
        } else {
            self.text.len()..self.text.len()
        }
    }

    /// Converts a UTF-8 byte offset to 0-indexed 2D `(line, column)` character coordinates.
    pub fn byte_to_2d(&self, byte_pos: usize) -> Position2D {
        let clamped = floor_char_boundary(&self.text, byte_pos.min(self.text.len()));
        let prefix = &self.text[..clamped];
        let line = prefix.bytes().filter(|&b| b == b'\n').count();
        let line_slice = prefix.rsplit('\n').next().unwrap_or(prefix);
        let clean_slice = line_slice.strip_suffix('\r').unwrap_or(line_slice);
        let column = clean_slice.chars().count();
        Position2D { line, column }
    }

    /// Converts 0-indexed 2D `(line, column)` character coordinates to a UTF-8 byte offset,
    /// clamping `column` to the line's character length.
    pub fn pos_2d_to_byte(&self, pos: Position2D) -> usize {
        let ranges = self.line_content_ranges();
        if ranges.is_empty() {
            return 0;
        }
        let clamped_line = pos.line.min(ranges.len() - 1);
        let range = &ranges[clamped_line];
        let line_str = &self.text[range.clone()];
        let byte_in_line: usize = line_str
            .chars()
            .take(pos.column)
            .map(|c| c.len_utf8())
            .sum();
        range.start + byte_in_line
    }

    /// Returns the character length of the 0-indexed `line_idx` (excluding `\r\n`).
    pub fn line_char_len(&self, line_idx: usize) -> usize {
        let range = self.line_range_bytes(line_idx);
        self.text[range].chars().count()
    }

    /// Collapses all secondary cursors and clears rectangular block selection (`Esc`).
    pub fn collapse_to_primary_cursor(&mut self) {
        self.cursors.remove_secondary();
        self.cursors.primary_mut().clear_block_selection();
    }

    /// Clears block selection mode on any cursor that has it active.
    pub fn clear_block_selection_if_active(&mut self) {
        for (_, cursor) in self.cursors.iter_mut() {
            if cursor.has_block_selection() {
                cursor.clear_block_selection();
                cursor.anchor = None;
            }
        }
    }

    /// Handles `Alt+Click` at `byte_pos`, toggling or adding a cursor at that offset.
    pub fn handle_alt_click(&mut self, byte_pos: usize) -> CursorId {
        let clamped = floor_char_boundary(&self.text, byte_pos.min(self.text.len()));
        self.clear_block_selection_if_active();
        self.cursors.toggle_at(clamped)
    }

    /// Handles `Alt+Drag` from `origin_byte` to `current_byte`, creating or updating a
    /// rectangular block selection (`SelectionMode::Block`).
    pub fn handle_alt_drag(&mut self, origin_byte: usize, current_byte: usize) {
        let origin_clamped = floor_char_boundary(&self.text, origin_byte.min(self.text.len()));
        let current_clamped = floor_char_boundary(&self.text, current_byte.min(self.text.len()));
        let origin_2d = self.byte_to_2d(origin_clamped);
        let current_2d = self.byte_to_2d(current_clamped);

        self.cursors.remove_secondary();
        let primary = self.cursors.primary_mut();
        primary.selection_mode = SelectionMode::Block;
        primary.block_anchor = Some(origin_2d);
        primary.anchor = Some(origin_clamped);
        primary.position = current_clamped;
        primary.sticky_column = Some(current_2d.column);
    }

    /// Adds a new cursor on the line directly above the topmost cursor (`Ctrl+Alt+Up`).
    pub fn add_cursor_above(&mut self) -> Option<CursorId> {
        self.clear_block_selection_if_active();
        let top_pos = self
            .cursors
            .iter()
            .map(|(_, c)| c.position)
            .min()
            .unwrap_or(0);
        let pos_2d = self.byte_to_2d(top_pos);
        if pos_2d.line == 0 {
            return None;
        }
        let target_2d = Position2D {
            line: pos_2d.line - 1,
            column: pos_2d.column,
        };
        let new_byte_pos = self.pos_2d_to_byte(target_2d);
        let mut cursor = Cursor::new(new_byte_pos);
        cursor.sticky_column = Some(pos_2d.column);
        let id = self.cursors.add(cursor);
        self.cursors.set_primary(id);
        Some(id)
    }

    /// Adds a new cursor on the line directly below the bottommost cursor (`Ctrl+Alt+Down`).
    pub fn add_cursor_below(&mut self) -> Option<CursorId> {
        self.clear_block_selection_if_active();
        let bottom_pos = self
            .cursors
            .iter()
            .map(|(_, c)| c.position)
            .max()
            .unwrap_or(0);
        let pos_2d = self.byte_to_2d(bottom_pos);
        let total_lines = self.line_count();
        if pos_2d.line + 1 >= total_lines {
            return None;
        }
        let target_2d = Position2D {
            line: pos_2d.line + 1,
            column: pos_2d.column,
        };
        let new_byte_pos = self.pos_2d_to_byte(target_2d);
        let mut cursor = Cursor::new(new_byte_pos);
        cursor.sticky_column = Some(pos_2d.column);
        let id = self.cursors.add(cursor);
        self.cursors.set_primary(id);
        Some(id)
    }

    /// Adds a cursor at the next occurrence of the current selection, or selects the word
    /// under the primary cursor if no selection exists (`Ctrl+D` / `add_cursor_at_next_match`).
    pub fn add_cursor_at_next_match(&mut self) -> Option<CursorId> {
        self.clear_block_selection_if_active();

        let primary_range = self.cursors.primary().selection_range();
        if primary_range.is_none() {
            // Select the word under the primary cursor first
            let pos = self.cursors.primary().position;
            if let Some(word_range) = self.word_range_at(pos) {
                let primary = self.cursors.primary_mut();
                primary.anchor = Some(word_range.start);
                primary.position = word_range.end;
                return Some(self.cursors.primary_id());
            }
            return None;
        }

        let range = primary_range?;
        let needle = self.text[range.clone()].to_string();
        if needle.is_empty() {
            return None;
        }

        // Collect existing selection ranges so we skip already-selected occurrences
        let existing_ranges: Vec<Range<usize>> = self
            .cursors
            .iter()
            .filter_map(|(_, c)| c.selection_range())
            .collect();

        let search_start = self
            .cursors
            .iter()
            .map(|(_, c)| c.selection_end())
            .max()
            .unwrap_or(range.end);

        // Search forward from `search_start`, then wrap around from the start of the buffer
        let next_match = self.find_unselected_match(&needle, search_start..self.text.len(), &existing_ranges)
            .or_else(|| self.find_unselected_match(&needle, 0..search_start, &existing_ranges))?;

        let new_cursor = Cursor::with_selection(next_match.start, next_match.end);
        let id = self.cursors.add(new_cursor);
        self.cursors.set_primary(id);
        Some(id)
    }

    fn find_unselected_match(
        &self,
        needle: &str,
        search_span: Range<usize>,
        existing_ranges: &[Range<usize>],
    ) -> Option<Range<usize>> {
        let mut offset = search_span.start;
        while offset + needle.len() <= search_span.end {
            let slice = &self.text[offset..search_span.end];
            if let Some(rel) = slice.find(needle) {
                let match_start = offset + rel;
                let match_end = match_start + needle.len();
                let candidate = match_start..match_end;
                if !existing_ranges.iter().any(|r| r == &candidate) {
                    return Some(candidate);
                }
                offset = match_start + needle.chars().next().map_or(1, |c| c.len_utf8());
            } else {
                break;
            }
        }
        None
    }

    fn word_range_at(&self, byte_pos: usize) -> Option<Range<usize>> {
        if self.text.is_empty() {
            return None;
        }
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
        let clamped = floor_char_boundary(&self.text, byte_pos.min(self.text.len()));

        let mut start = clamped;
        for (idx, ch) in self.text[..clamped].char_indices().rev() {
            if is_word_char(ch) {
                start = idx;
            } else {
                break;
            }
        }

        let mut end = clamped;
        for (rel_idx, ch) in self.text[clamped..].char_indices() {
            if is_word_char(ch) {
                end = clamped + rel_idx + ch.len_utf8();
            } else {
                break;
            }
        }

        if start < end {
            Some(start..end)
        } else {
            None
        }
    }

    /// Splits multi-line selection(s) into one cursor at the end of each selected line (`Alt+Shift+I`).
    pub fn add_cursors_to_line_ends(&mut self) {
        if self.cursors.has_block_selection() {
            let _ = self.convert_block_selection_to_cursors();
            for (_, c) in self.cursors.iter_mut() {
                let end = c.selection_end();
                c.anchor = None;
                c.position = end;
            }
            self.cursors.normalize();
            return;
        }

        let ranges = self.line_content_ranges();
        let mut target_positions: Vec<usize> = Vec::new();

        for (_, cursor) in self.cursors.iter() {
            if let Some(sel) = cursor.selection_range() {
                let start_line = self.byte_to_2d(sel.start).line;
                let end_line = self.byte_to_2d(sel.end).line;
                for line_idx in start_line..=end_line {
                    if let Some(line_range) = ranges.get(line_idx) {
                        target_positions.push(line_range.end);
                    }
                }
            } else {
                let line_idx = self.byte_to_2d(cursor.position).line;
                if let Some(line_range) = ranges.get(line_idx) {
                    target_positions.push(line_range.end);
                }
            }
        }

        if !target_positions.is_empty() {
            let mut new_cursors = Cursors::from_single(Cursor::new(target_positions[0]));
            for &pos in &target_positions[1..] {
                let id = new_cursors.add_direct(Cursor::new(pos));
                new_cursors.set_primary(id);
            }
            new_cursors.normalize();
            self.cursors = new_cursors;
        }
    }

    /// Extends or modifies a rectangular block selection in `direction` (`Shift+Alt+Arrows`).
    /// Mirrors `block_select_action` in `fresh-editor/src/input/actions.rs`.
    pub fn block_select(&mut self, direction: BlockDirection) {
        let total_lines = self.line_count();
        let primary_pos = self.cursors.primary().position;
        let current_2d = self.byte_to_2d(primary_pos);

        // Initialize block selection if not already in Block mode
        let anchor_2d = if let Some(existing_anchor) = self.cursors.primary().block_anchor {
            if self.cursors.primary().selection_mode == SelectionMode::Block {
                existing_anchor
            } else {
                current_2d
            }
        } else {
            current_2d
        };

        let active_col = self
            .cursors
            .primary()
            .sticky_column
            .unwrap_or(current_2d.column);

        let new_2d = match direction {
            BlockDirection::Left => Position2D {
                line: current_2d.line,
                column: active_col.saturating_sub(1),
            },
            BlockDirection::Right => Position2D {
                line: current_2d.line,
                column: active_col + 1,
            },
            BlockDirection::Up => Position2D {
                line: current_2d.line.saturating_sub(1),
                column: active_col,
            },
            BlockDirection::Down => Position2D {
                line: (current_2d.line + 1).min(total_lines.saturating_sub(1)),
                column: active_col,
            },
        };

        let anchor_byte = self.pos_2d_to_byte(anchor_2d);
        let new_byte = self.pos_2d_to_byte(new_2d);

        self.cursors.remove_secondary();
        let primary = self.cursors.primary_mut();
        primary.selection_mode = SelectionMode::Block;
        primary.block_anchor = Some(anchor_2d);
        primary.anchor = Some(anchor_byte);
        primary.position = new_byte;
        primary.sticky_column = Some(new_2d.column);
    }

    /// Converts an active rectangular block selection into one standard `Cursor` per spanned line
    /// before performing an edit (`InsertChar`, `DeleteBackward`, `DeleteForward`, `Cut`, `Paste`).
    ///
    /// Mirrors `convert_block_selection_to_cursors` in `fresh-editor/src/input/actions.rs`.
    /// Returns a list of `(CursorId, virtual_space_padding)` for lines shorter than `min_col`.
    pub fn convert_block_selection_to_cursors(&mut self) -> Vec<(CursorId, usize)> {
        let mut block_pads: Vec<(CursorId, usize)> = Vec::new();
        let Some(block_anchor) = self.cursors.primary().block_anchor else {
            return block_pads;
        };
        if self.cursors.primary().selection_mode != SelectionMode::Block {
            return block_pads;
        }

        let current_pos = self.cursors.primary().position;
        let mut current_2d = self.byte_to_2d(current_pos);
        if let Some(sticky_col) = self.cursors.primary().sticky_column {
            current_2d.column = sticky_col;
        }

        let min_line = block_anchor.line.min(current_2d.line);
        let max_line = block_anchor.line.max(current_2d.line);
        let min_col = block_anchor.column.min(current_2d.column);
        let max_col = block_anchor.column.max(current_2d.column);

        let ranges = self.line_content_ranges();
        let mut first = true;

        for line_idx in min_line..=max_line {
            if line_idx >= ranges.len() {
                break;
            }
            let line_range = &ranges[line_idx];
            let line_str = &self.text[line_range.clone()];
            let line_chars: Vec<char> = line_str.chars().collect();
            let line_char_len = line_chars.len();

            let actual_min_col = min_col.min(line_char_len);
            let actual_max_col = max_col.min(line_char_len);
            let pad = min_col.saturating_sub(line_char_len);

            let start_byte_rel: usize = line_chars[..actual_min_col]
                .iter()
                .map(|c| c.len_utf8())
                .sum();
            let end_byte_rel: usize = line_chars[..actual_max_col]
                .iter()
                .map(|c| c.len_utf8())
                .sum();

            let sel_start = line_range.start + start_byte_rel;
            let sel_end = line_range.start + end_byte_rel;

            if first {
                first = false;
                let primary_id = self.cursors.primary_id();
                let primary = self.cursors.primary_mut();
                primary.clear_block_selection();
                primary.position = sel_end;
                primary.anchor = if sel_start == sel_end {
                    None
                } else {
                    Some(sel_start)
                };
                if pad > 0 {
                    block_pads.push((primary_id, pad));
                }
            } else {
                let cursor = Cursor::with_selection(sel_start, sel_end);
                let new_id = self.cursors.add_direct(cursor);
                if pad > 0 {
                    block_pads.push((new_id, pad));
                }
            }
        }

        self.cursors.normalize();
        block_pads
    }

    /// Moves all cursors in `direction`, optionally extending selections (`Shift+Arrows/Home/End`).
    pub fn move_cursors(&mut self, direction: MoveDirection, extend_selection: bool) {
        if !extend_selection {
            self.clear_block_selection_if_active();
        } else if self.cursors.has_block_selection() {
            let _ = self.convert_block_selection_to_cursors();
        }

        let ranges = self.line_content_ranges();
        let total_lines = ranges.len();

        // Snapshot cursor states first to avoid simultaneous borrow of `self` and `self.cursors`
        let snapshots: Vec<(CursorId, Cursor)> = self
            .cursors
            .iter()
            .map(|(id, c)| (id, c.clone()))
            .collect();

        let mut updated: Vec<(CursorId, Cursor)> = Vec::with_capacity(snapshots.len());

        for (id, mut cursor) in snapshots {
            match direction {
                MoveDirection::Left => {
                    if !extend_selection && cursor.deselect_on_move {
                        if let Some(range) = cursor.selection_range() {
                            cursor.move_to(range.start, false);
                            cursor.sticky_column = None;
                            updated.push((id, cursor));
                            continue;
                        }
                    }
                    let new_pos = self.prev_grapheme_boundary(cursor.position);
                    cursor.move_to(new_pos, extend_selection);
                    cursor.sticky_column = None;
                }
                MoveDirection::Right => {
                    if !extend_selection && cursor.deselect_on_move {
                        if let Some(range) = cursor.selection_range() {
                            cursor.move_to(range.end, false);
                            cursor.sticky_column = None;
                            updated.push((id, cursor));
                            continue;
                        }
                    }
                    let new_pos = self.next_grapheme_boundary(cursor.position);
                    cursor.move_to(new_pos, extend_selection);
                    cursor.sticky_column = None;
                }
                MoveDirection::Up => {
                    let pos_2d = self.byte_to_2d(cursor.position);
                    let sticky = cursor.sticky_column.unwrap_or(pos_2d.column);
                    if pos_2d.line > 0 {
                        let target = Position2D {
                            line: pos_2d.line - 1,
                            column: sticky,
                        };
                        let new_pos = self.pos_2d_to_byte(target);
                        cursor.move_to(new_pos, extend_selection);
                        cursor.sticky_column = Some(sticky);
                    } else {
                        cursor.move_to(0, extend_selection);
                        cursor.sticky_column = Some(sticky);
                    }
                }
                MoveDirection::Down => {
                    let pos_2d = self.byte_to_2d(cursor.position);
                    let sticky = cursor.sticky_column.unwrap_or(pos_2d.column);
                    if pos_2d.line + 1 < total_lines {
                        let target = Position2D {
                            line: pos_2d.line + 1,
                            column: sticky,
                        };
                        let new_pos = self.pos_2d_to_byte(target);
                        cursor.move_to(new_pos, extend_selection);
                        cursor.sticky_column = Some(sticky);
                    } else {
                        cursor.move_to(self.text.len(), extend_selection);
                        cursor.sticky_column = Some(sticky);
                    }
                }
                MoveDirection::Home => {
                    let pos_2d = self.byte_to_2d(cursor.position);
                    let line_start = ranges.get(pos_2d.line).map_or(0, |r| r.start);
                    cursor.move_to(line_start, extend_selection);
                    cursor.sticky_column = Some(0);
                }
                MoveDirection::End => {
                    let pos_2d = self.byte_to_2d(cursor.position);
                    let line_end = ranges
                        .get(pos_2d.line)
                        .map_or(self.text.len(), |r| r.end);
                    cursor.move_to(line_end, extend_selection);
                    let end_col = self.byte_to_2d(line_end).column;
                    cursor.sticky_column = Some(end_col);
                }
            }
            updated.push((id, cursor));
        }

        for (id, new_cursor) in updated {
            if let Some(c) = self.cursors.get_mut(id) {
                *c = new_cursor;
            }
        }
        self.cursors.normalize();
    }

    fn prev_grapheme_boundary(&self, pos: usize) -> usize {
        let clamped = floor_char_boundary(&self.text, pos.min(self.text.len()));
        if clamped == 0 {
            return 0;
        }
        let bytes = self.text.as_bytes();
        if clamped >= 2 && bytes[clamped - 2] == b'\r' && bytes[clamped - 1] == b'\n' {
            return clamped - 2;
        }
        self.text[..clamped]
            .char_indices()
            .next_back()
            .map_or(0, |(idx, _)| idx)
    }

    fn next_grapheme_boundary(&self, pos: usize) -> usize {
        let clamped = floor_char_boundary(&self.text, pos.min(self.text.len()));
        if clamped >= self.text.len() {
            return self.text.len();
        }
        let bytes = self.text.as_bytes();
        if clamped + 1 < bytes.len() && bytes[clamped] == b'\r' && bytes[clamped + 1] == b'\n' {
            return clamped + 2;
        }
        let Some(ch) = self.text[clamped..].chars().next() else {
            return clamped;
        };
        clamped + ch.len_utf8()
    }

    /// Inserts `insert_str` at every active cursor (replacing any active selections or block selection).
    ///
    /// Uses reverse-order (descending byte offset) buffer mutation and ascending prefix-summed
    /// cursor offset rebasing to satisfy Rust's borrow checker and keep all cursor offsets exact.
    pub fn insert_text(&mut self, insert_str: &str) {
        self.save_undo_snapshot();
        let block_pads = self.convert_block_selection_to_cursors();

        #[derive(Clone)]
        struct PendingEdit {
            cursor_id: CursorId,
            replace_range: Range<usize>,
            replacement: String,
        }

        // 1. Snapshot all cursor edit specifications before mutating `self.text`
        let mut edits: Vec<PendingEdit> = self
            .cursors
            .iter()
            .map(|(id, cursor)| {
                let replace_range = cursor
                    .selection_range()
                    .unwrap_or(cursor.position..cursor.position);
                let pad = block_pads
                    .iter()
                    .find(|(cid, _)| *cid == id)
                    .map(|(_, p)| *p)
                    .unwrap_or(0);
                let replacement = if pad > 0 && replace_range.is_empty() {
                    format!("{}{}", " ".repeat(pad), insert_str)
                } else {
                    insert_str.to_string()
                };
                PendingEdit {
                    cursor_id: id,
                    replace_range,
                    replacement,
                }
            })
            .collect();

        // 2. Sort descending by start offset so applying edits from back-to-front
        //    never invalidates earlier byte offsets in `self.text`.
        edits.sort_by_key(|e| Reverse(e.replace_range.start));

        for edit in &edits {
            self.text
                .replace_range(edit.replace_range.clone(), &edit.replacement);
        }

        // 3. Sort ascending by start offset and compute post-edit cursor positions
        edits.sort_by_key(|e| e.replace_range.start);
        let mut cumulative_delta: isize = 0;

        for edit in edits {
            let old_len = edit.replace_range.len() as isize;
            let new_len = edit.replacement.len() as isize;
            let shifted_start = (edit.replace_range.start as isize + cumulative_delta).max(0) as usize;
            let new_cursor_pos = shifted_start + edit.replacement.len();
            cumulative_delta += new_len - old_len;

            if let Some(cursor) = self.cursors.get_mut(edit.cursor_id) {
                cursor.position = new_cursor_pos;
                cursor.anchor = None;
                cursor.clear_block_selection();
                cursor.sticky_column = None;
            }
        }

        self.cursors.normalize();
    }

    /// Deletes one character backward (or active selection) at every cursor (`Backspace`).
    pub fn delete_backward(&mut self) {
        self.save_undo_snapshot();
        let _ = self.convert_block_selection_to_cursors();

        let mut edits: Vec<(CursorId, Range<usize>)> = self
            .cursors
            .iter()
            .map(|(id, cursor)| {
                if let Some(sel) = cursor.selection_range() {
                    (id, sel)
                } else {
                    let prev = self.prev_grapheme_boundary(cursor.position);
                    (id, prev..cursor.position)
                }
            })
            .collect();

        // Deduplicate or merge overlapping deletion ranges
        edits.sort_by_key(|(_, r)| Reverse(r.start));
        for (_, range) in &edits {
            if !range.is_empty() && range.end <= self.text.len() {
                self.text.replace_range(range.clone(), "");
            }
        }

        edits.sort_by_key(|(_, r)| r.start);
        let mut cumulative_deleted: usize = 0;
        for (id, range) in edits {
            let new_pos = range.start.saturating_sub(cumulative_deleted);
            cumulative_deleted += range.len();
            if let Some(cursor) = self.cursors.get_mut(id) {
                cursor.position = new_pos;
                cursor.anchor = None;
                cursor.clear_block_selection();
                cursor.sticky_column = None;
            }
        }

        self.cursors.normalize();
    }

    /// Deletes one character forward (or active selection) at every cursor (`Delete`).
    pub fn delete_forward(&mut self) {
        self.save_undo_snapshot();
        let _ = self.convert_block_selection_to_cursors();

        let mut edits: Vec<(CursorId, Range<usize>)> = self
            .cursors
            .iter()
            .map(|(id, cursor)| {
                if let Some(sel) = cursor.selection_range() {
                    (id, sel)
                } else {
                    let next = self.next_grapheme_boundary(cursor.position);
                    (id, cursor.position..next)
                }
            })
            .collect();

        edits.sort_by_key(|(_, r)| Reverse(r.start));
        for (_, range) in &edits {
            if !range.is_empty() && range.end <= self.text.len() {
                self.text.replace_range(range.clone(), "");
            }
        }

        edits.sort_by_key(|(_, r)| r.start);
        let mut cumulative_deleted: usize = 0;
        for (id, range) in edits {
            let new_pos = range.start.saturating_sub(cumulative_deleted);
            cumulative_deleted += range.len();
            if let Some(cursor) = self.cursors.get_mut(id) {
                cursor.position = new_pos;
                cursor.anchor = None;
                cursor.clear_block_selection();
                cursor.sticky_column = None;
            }
        }

        self.cursors.normalize();
    }

    /// Copies the selected text across block selection or multiple cursors (`Ctrl+C`).
    /// Mirrors `copy_selection` and `copy_block_selection_text` in `fresh-editor/src/app/clipboard.rs`.
    pub fn copy_selection(&self) -> Option<String> {
        let primary = self.cursors.primary();
        if primary.has_block_selection() {
            let block_anchor = primary.block_anchor?;
            let mut current_2d = self.byte_to_2d(primary.position);
            if let Some(sticky_col) = primary.sticky_column {
                current_2d.column = sticky_col;
            }
            let min_line = block_anchor.line.min(current_2d.line);
            let max_line = block_anchor.line.max(current_2d.line);
            let min_col = block_anchor.column.min(current_2d.column);
            let max_col = block_anchor.column.max(current_2d.column);

            if min_col == max_col {
                return None;
            }

            let ranges = self.line_content_ranges();
            let mut lines_out = Vec::new();
            for line_idx in min_line..=max_line {
                if let Some(r) = ranges.get(line_idx) {
                    let line_chars: Vec<char> = self.text[r.clone()].chars().collect();
                    let sc = min_col.min(line_chars.len());
                    let ec = max_col.min(line_chars.len());
                    let slice: String = line_chars[sc..ec].iter().collect();
                    lines_out.push(slice);
                }
            }
            return Some(lines_out.join("\r\n"));
        }

        let mut pieces: Vec<&str> = Vec::new();
        for (_, cursor) in self.cursors.iter() {
            if let Some(range) = cursor.selection_range() {
                pieces.push(&self.text[range]);
            }
        }
        if pieces.is_empty() {
            None
        } else {
            Some(pieces.join("\r\n"))
        }
    }

    /// Cuts the selected text across block selection or multiple cursors (`Ctrl+X`).
    pub fn cut_selection(&mut self) -> Option<String> {
        let copied = self.copy_selection()?;
        self.delete_backward();
        Some(copied)
    }

    /// Pastes `clipboard_text` into the buffer (`Ctrl+V`).
    /// If the number of lines in `clipboard_text` matches the number of active cursors (> 1),
    /// distributes one line to each cursor in document order; otherwise inserts `clipboard_text` at each cursor.
    pub fn paste_text(&mut self, clipboard_text: &str) {
        self.save_undo_snapshot();
        let block_pads = self.convert_block_selection_to_cursors();

        let lines: Vec<&str> = clipboard_text.lines().collect();
        let distribute_lines = self.cursors.count() > 1 && lines.len() == self.cursors.count();

        let mut edits: Vec<(CursorId, Range<usize>, String)> = self
            .cursors
            .iter()
            .enumerate()
            .map(|(idx, (id, cursor))| {
                let replace_range = cursor
                    .selection_range()
                    .unwrap_or(cursor.position..cursor.position);
                let pad = block_pads
                    .iter()
                    .find(|(cid, _)| *cid == id)
                    .map(|(_, p)| *p)
                    .unwrap_or(0);
                let raw_piece = if distribute_lines {
                    lines[idx].to_string()
                } else {
                    clipboard_text.to_string()
                };
                let replacement = if pad > 0 && replace_range.is_empty() {
                    format!("{}{}", " ".repeat(pad), raw_piece)
                } else {
                    raw_piece
                };
                (id, replace_range, replacement)
            })
            .collect();

        edits.sort_by_key(|(_, r, _)| Reverse(r.start));
        for (_, range, replacement) in &edits {
            self.text.replace_range(range.clone(), replacement);
        }

        edits.sort_by_key(|(_, r, _)| r.start);
        let mut cumulative_delta: isize = 0;
        for (id, range, replacement) in edits {
            let old_len = range.len() as isize;
            let new_len = replacement.len() as isize;
            let shifted_start = (range.start as isize + cumulative_delta).max(0) as usize;
            let new_pos = shifted_start + replacement.len();
            cumulative_delta += new_len - old_len;

            if let Some(cursor) = self.cursors.get_mut(id) {
                cursor.position = new_pos;
                cursor.anchor = None;
                cursor.clear_block_selection();
                cursor.sticky_column = None;
            }
        }

        self.cursors.normalize();
    }

    /// Computes the `SelectionContext` for rendering multiple carets, secondary selections,
    /// and rectangular block selections (`selection_context` in SRC).
    pub fn selection_context(&self) -> SelectionContext {
        let mut ranges: Vec<Range<usize>> = self
            .cursors
            .iter()
            .filter(|(_, c)| c.selection_mode != SelectionMode::Block)
            .filter_map(|(_, c)| c.selection_range())
            .collect();
        ranges.sort_by_key(|r| r.start);

        let mut block_rects: Vec<(usize, usize, usize, usize)> = Vec::new();
        for (_, cursor) in self.cursors.iter() {
            if cursor.selection_mode == SelectionMode::Block {
                if let Some(block_anchor) = cursor.block_anchor {
                    let mut cur_2d = self.byte_to_2d(cursor.position);
                    if let Some(sticky_col) = cursor.sticky_column {
                        cur_2d.column = sticky_col;
                    }
                    let start_line = block_anchor.line.min(cur_2d.line);
                    let end_line = block_anchor.line.max(cur_2d.line);
                    let start_col = block_anchor.column.min(cur_2d.column);
                    let end_col = block_anchor.column.max(cur_2d.column);
                    block_rects.push((start_line, start_col, end_line, end_col));
                }
            }
        }

        let cursor_positions: Vec<usize> = self.cursors.iter().map(|(_, c)| c.position).collect();
        let primary_cursor_position = self.cursors.primary().position;

        SelectionContext {
            ranges,
            block_rects,
            cursor_positions,
            primary_cursor_position,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_cursor_add_above_below_and_insert() {
        let mut editor = MultiCursorEditor::with_text("alpha\r\nbeta\r\ngamma");
        // Move primary cursor to column 2 on line 1 ("be|ta")
        let pos = editor.pos_2d_to_byte(Position2D { line: 1, column: 2 });
        editor.cursors.primary_mut().move_to(pos, false);

        editor.add_cursor_above();
        editor.add_cursor_below();
        assert_eq!(editor.cursors.count(), 3);

        editor.insert_text("_X_");
        assert_eq!(editor.text(), "al_X_pha\r\nbe_X_ta\r\nga_X_mma");
    }

    #[test]
    fn test_ctrl_d_add_cursor_at_next_match() {
        let mut editor = MultiCursorEditor::with_text("foo bar foo baz foo");
        // First Ctrl+D selects "foo" at offset 0..3
        editor.add_cursor_at_next_match();
        assert_eq!(editor.cursors.count(), 1);
        assert_eq!(editor.cursors.primary().selection_range(), Some(0..3));

        // Second Ctrl+D adds second cursor at 8..11
        editor.add_cursor_at_next_match();
        assert_eq!(editor.cursors.count(), 2);

        // Third Ctrl+D adds third cursor at 16..19
        editor.add_cursor_at_next_match();
        assert_eq!(editor.cursors.count(), 3);

        // Typing replaces all three "foo" occurrences simultaneously
        editor.insert_text("qux");
        assert_eq!(editor.text(), "qux bar qux baz qux");
    }

    #[test]
    fn test_block_selection_and_conversion_to_cursors() {
        let mut editor = MultiCursorEditor::with_text("line100\r\nline200\r\nline300");
        let start = editor.pos_2d_to_byte(Position2D { line: 0, column: 4 });
        editor.cursors.primary_mut().move_to(start, false);

        // Shift+Alt+Down twice and Shift+Alt+Right three times selects "100", "200", "300"
        editor.block_select(BlockDirection::Down);
        editor.block_select(BlockDirection::Down);
        editor.block_select(BlockDirection::Right);
        editor.block_select(BlockDirection::Right);
        editor.block_select(BlockDirection::Right);

        assert!(editor.cursors.has_block_selection());
        assert_eq!(
            editor.copy_selection().as_deref(),
            Some("100\r\n200\r\n300")
        );

        // Typing decomposes the rectangular block into 3 per-line cursors and replaces "100"/"200"/"300"
        editor.insert_text("_OK");
        assert!(!editor.cursors.has_block_selection());
        assert_eq!(editor.cursors.count(), 3);
        assert_eq!(editor.text(), "line_OK\r\nline_OK\r\nline_OK");
    }

    #[test]
    fn test_selection_active_set_sweep() {
        let mut editor = MultiCursorEditor::with_text("abcd\r\nefgh\r\nijkl");
        editor.handle_alt_drag(
            editor.pos_2d_to_byte(Position2D { line: 0, column: 1 }),
            editor.pos_2d_to_byte(Position2D { line: 2, column: 3 }),
        );
        let ctx = editor.selection_context();
        assert_eq!(ctx.block_rects, vec![(0, 1, 2, 3)]);

        let mut sweep = SelectionActiveSet::new(&ctx);
        sweep.enter_line(1);
        assert!(!sweep.contains(6, 0));
        assert!(sweep.contains(7, 1));
        assert!(sweep.contains(8, 2));
        assert!(!sweep.contains(9, 3));
    }
}
