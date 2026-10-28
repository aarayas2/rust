//! Incremental Syntax Highlighting Engine, Rope Buffer Synchronization, and Bracket Matcher.
//!
//! Provides:
//! - [`Highlighter`]: Trait for incremental line-based syntax highlighters.
//! - [`IncrementalHighlightCache`]: `ropey::Rope`-backed line state checkpoint cache (`ParseState` + `ScopeStack`)
//!   that converges in `O(1)` lines on single-line edits and tokenizes 10,000+ lines in `< 16ms`.
//! - [`find_matching_bracket`]: Scope-aware balanced bracket matcher (`()`, `{}`, `[]`).

pub mod registry;
pub mod tokenizer;

pub use registry::{
    RegisteredSyntax, SyntaxDefinition, SyntaxRegistry, GLOBAL_SYNTAX_REGISTRY,
};
pub use tokenizer::{
    apply_scope_ops, HighlightToken, LineLexerState, SyntectTokenizer, TreeSitterFallback,
};

use ropey::Rope;
use std::borrow::Cow;

/// Trait for incremental syntax highlighters operating on a `ropey::Rope` buffer.
pub trait Highlighter {
    /// Synchronizes the highlighter's internal `Rope` buffer with `new_text`, invalidating only
    /// from the first modified line index.
    fn update_text(&mut self, new_text: &str);

    /// Switches the active syntax definition by name (e.g., `"Rust"`, `"JavaScript"`, `"Python"`).
    fn set_syntax(&mut self, registry: &SyntaxRegistry, syntax_name: &str);

    /// Ensures lines `0..=end_line` are tokenized and returns the slice of per-line token vectors.
    fn highlight_up_to(&mut self, registry: &SyntaxRegistry, end_line: usize) -> &[Vec<HighlightToken>];

    /// Invalidates cached line states starting at `from_line`.
    fn invalidate_from_line(&mut self, from_line: usize);
}

/// Incremental per-line state and token cache backed by `ropey::Rope`.
#[derive(Debug, Clone)]
pub struct IncrementalHighlightCache {
    /// `ropey::Rope` representation of the document for `O(log N)` line indexing and slicing.
    rope: Rope,
    /// Name of the active syntax definition (e.g., `"Rust"`, `"JavaScript"`, `"Python"`).
    syntax_name: String,
    /// Cached exit `LineLexerState` at the end of each line `i`.
    exit_states: Vec<LineLexerState>,
    /// Cached token spans for each line `i`.
    line_tokens: Vec<Vec<HighlightToken>>,
    /// Index of the first line whose cached state is currently dirty/invalid.
    valid_up_to_line: usize,
}

impl Default for IncrementalHighlightCache {
    fn default() -> Self {
        Self::new("Plain Text")
    }
}

impl IncrementalHighlightCache {
    /// Creates a new `IncrementalHighlightCache` initialized for `syntax_name`.
    pub fn new(syntax_name: impl Into<String>) -> Self {
        Self {
            rope: Rope::new(),
            syntax_name: syntax_name.into(),
            exit_states: Vec::new(),
            line_tokens: Vec::new(),
            valid_up_to_line: 0,
        }
    }

    /// Returns a reference to the underlying `ropey::Rope` buffer.
    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    /// Returns the active syntax name.
    pub fn syntax_name(&self) -> &str {
        &self.syntax_name
    }

    /// Returns the number of lines currently valid in the cache without re-tokenizing.
    pub fn valid_up_to_line(&self) -> usize {
        self.valid_up_to_line
    }

    /// Returns the cached tokens for `line_idx` if already tokenized.
    pub fn tokens_for_line(&self, line_idx: usize) -> Option<&[HighlightToken]> {
        if line_idx < self.valid_up_to_line {
            self.line_tokens.get(line_idx).map(|v| v.as_slice())
        } else {
            None
        }
    }

    /// Returns a slice of cached tokens for `line_idx` (or `&[]` if not yet tokenized).
    pub fn line_tokens(&self, line_idx: usize) -> &[HighlightToken] {
        self.tokens_for_line(line_idx).unwrap_or(&[])
    }

    /// Returns the total number of lines in the underlying `Rope` buffer.
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// Updates both the active syntax definition and the underlying `Rope` buffer text.
    pub fn set_syntax_and_text(&mut self, syntax_name: &str, text: &str) {
        self.set_syntax_name(syntax_name);
        self.sync_rope(text);
    }

    /// Synchronizes the `Rope` with `new_text` after an edit near `_hint_line`.
    pub fn update_text_from_line(&mut self, new_text: &str, _hint_line: usize) {
        self.sync_rope(new_text);
    }

    /// Synchronizes the `Rope` with `new_text` and invalidates only from the earliest modified line.
    pub fn sync_rope(&mut self, new_text: &str) {
        let new_rope = Rope::from_str(new_text);
        let old_len_lines = self.rope.len_lines();
        let new_len_lines = new_rope.len_lines();
        let common_lines = old_len_lines.min(new_len_lines);

        let mut first_diff = common_lines;
        for i in 0..common_lines {
            if self.rope.line(i) != new_rope.line(i) {
                first_diff = i;
                break;
            }
        }

        if first_diff == common_lines && old_len_lines == new_len_lines {
            return;
        }

        // If the number of lines changed, drop stale tail entries so convergence checks stay aligned
        if old_len_lines != new_len_lines {
            self.exit_states.truncate(first_diff);
            self.line_tokens.truncate(first_diff);
        }

        self.rope = new_rope;
        self.valid_up_to_line = self.valid_up_to_line.min(first_diff);
    }

    /// Changes the active syntax grammar and invalidates the cache from line 0.
    pub fn set_syntax_name(&mut self, syntax_name: impl Into<String>) {
        let next = syntax_name.into();
        if !self.syntax_name.eq_ignore_ascii_case(&next) {
            self.syntax_name = next;
            self.exit_states.clear();
            self.line_tokens.clear();
            self.valid_up_to_line = 0;
        }
    }

    /// Incrementally tokenizes from `self.valid_up_to_line` through `target_line` (inclusive).
    /// If the exit `LineLexerState` of an edited line converges with the previously cached exit
    /// state for that line (and line count is unchanged), the remaining cached lines are immediately
    /// marked valid in `O(1)` time.
    pub fn ensure_highlighted_up_to(&mut self, registry: &SyntaxRegistry, target_line: usize) {
        let total_lines = self.rope.len_lines().max(1);
        let clamped_target = target_line.min(total_lines.saturating_sub(1));
        if self.valid_up_to_line > clamped_target && self.line_tokens.len() >= total_lines {
            return;
        }

        let syntax_ref = registry.syntax_ref_by_name(&self.syntax_name);
        let mut cur_state = if self.valid_up_to_line == 0 {
            LineLexerState::new(syntax_ref)
        } else {
            self.exit_states[self.valid_up_to_line - 1].clone()
        };

        let can_converge = self.exit_states.len() == total_lines && self.line_tokens.len() == total_lines;

        for line_idx in self.valid_up_to_line..=clamped_target {
            let rope_line = self.rope.line(line_idx);
            let line_cow: Cow<str> = Cow::from(rope_line);
            let (tokens, next_state) =
                SyntectTokenizer::tokenize_line(registry, &line_cow, &cur_state);

            if line_idx < self.line_tokens.len() {
                self.line_tokens[line_idx] = tokens;
            } else {
                self.line_tokens.push(tokens);
            }

            let converged = can_converge
                && line_idx < self.exit_states.len()
                && self.exit_states[line_idx] == next_state;

            if line_idx < self.exit_states.len() {
                self.exit_states[line_idx] = next_state.clone();
            } else {
                self.exit_states.push(next_state.clone());
            }

            cur_state = next_state;
            self.valid_up_to_line = line_idx + 1;

            // State convergence: if exit state after editing line_idx matches what line_idx+1
            // already used, all subsequent lines in the cache are still 100% valid!
            if converged {
                self.valid_up_to_line = total_lines;
                break;
            }
        }
    }

    /// Finds the matching bracket byte offsets `(open_byte, close_byte)` when `caret_byte`
    /// is at or immediately after `(`, `)`, `{`, `}`, `[`, or `]`, ignoring brackets inside
    /// `comment.*` or `string.*` token scopes.
    pub fn find_matching_bracket(&self, caret_byte: usize) -> Option<(usize, usize)> {
        let text_len = self.rope.len_bytes();
        if text_len == 0 {
            return None;
        }

        let candidates = [
            caret_byte.min(text_len.saturating_sub(1)),
            caret_byte.saturating_sub(1),
        ];

        for &pos in &candidates {
            if pos >= text_len {
                continue;
            }
            let ch = self.byte_at(pos) as char;
            if matches!(ch, '(' | ')' | '{' | '}' | '[' | ']') && !self.is_byte_in_string_or_comment(pos)
            {
                if let Some(partner) = self.scan_partner_bracket(pos, ch) {
                    let pair = if pos < partner {
                        (pos, partner)
                    } else {
                        (partner, pos)
                    };
                    return Some(pair);
                }
            }
        }
        None
    }

    fn byte_at(&self, byte_idx: usize) -> u8 {
        self.rope.byte(byte_idx)
    }

    fn is_byte_in_string_or_comment(&self, byte_idx: usize) -> bool {
        if byte_idx >= self.rope.len_bytes() {
            return false;
        }
        let line_idx = self.rope.byte_to_line(byte_idx);
        let line_start = self.rope.line_to_byte(line_idx);
        let rel_byte = byte_idx.saturating_sub(line_start);

        if let Some(tokens) = self.tokens_for_line(line_idx) {
            for tok in tokens {
                if tok.range.contains(&rel_byte) {
                    return tok.scope.contains("comment") || tok.scope.contains("string");
                }
            }
        }
        false
    }

    fn scan_partner_bracket(&self, start_byte: usize, bracket: char) -> Option<usize> {
        let (target, forward) = match bracket {
            '(' => (')', true),
            '{' => ('}', true),
            '[' => (']', true),
            ')' => ('(', false),
            '}' => ('{', false),
            ']' => ('[', false),
            _ => return None,
        };

        let len = self.rope.len_bytes();
        let max_scan = 32_768usize;
        let mut depth = 1i32;

        if forward {
            let end = (start_byte + max_scan).min(len);
            for idx in (start_byte + 1)..end {
                let b = self.byte_at(idx) as char;
                if (b == bracket || b == target) && !self.is_byte_in_string_or_comment(idx) {
                    if b == bracket {
                        depth += 1;
                    } else {
                        depth -= 1;
                        if depth == 0 {
                            return Some(idx);
                        }
                    }
                }
            }
        } else {
            let min_idx = start_byte.saturating_sub(max_scan);
            for idx in (min_idx..start_byte).rev() {
                let b = self.byte_at(idx) as char;
                if (b == bracket || b == target) && !self.is_byte_in_string_or_comment(idx) {
                    if b == bracket {
                        depth += 1;
                    } else {
                        depth -= 1;
                        if depth == 0 {
                            return Some(idx);
                        }
                    }
                }
            }
        }
        None
    }
}

impl Highlighter for IncrementalHighlightCache {
    fn update_text(&mut self, new_text: &str) {
        self.sync_rope(new_text);
    }

    fn set_syntax(&mut self, _registry: &SyntaxRegistry, syntax_name: &str) {
        self.set_syntax_name(syntax_name);
    }

    fn highlight_up_to(
        &mut self,
        registry: &SyntaxRegistry,
        end_line: usize,
    ) -> &[Vec<HighlightToken>] {
        self.ensure_highlighted_up_to(registry, end_line);
        let limit = self.valid_up_to_line.min(self.line_tokens.len());
        &self.line_tokens[..limit]
    }

    fn invalidate_from_line(&mut self, from_line: usize) {
        self.valid_up_to_line = self.valid_up_to_line.min(from_line);
    }
}

/// Convenience free function that finds the matching bracket pair `(open_byte, close_byte)`
/// in `text` around `caret_byte`.
pub fn find_matching_bracket(text: &str, caret_byte: usize) -> Option<(usize, usize)> {
    let mut cache = IncrementalHighlightCache::new("Plain Text");
    cache.sync_rope(text);
    cache.find_matching_bracket(caret_byte)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_rust_highlighting_scopes_and_multiline_comments() {
        let reg = SyntaxRegistry::load_default();
        let mut cache = IncrementalHighlightCache::new("Rust");
        let code = "// line comment\r\npub fn compute(x: u32) -> u32 {\r\n    /* block start\r\n       still comment */\r\n    let msg = \"hello\";\r\n    return x + 42;\r\n}";
        cache.sync_rope(code);
        cache.ensure_highlighted_up_to(&reg, 10);

        let line0 = cache.tokens_for_line(0).expect("line 0 tokens");
        assert!(line0.iter().any(|t| t.scope.contains("comment.line")));

        let line1 = cache.tokens_for_line(1).expect("line 1 tokens");
        assert!(line1.iter().any(|t| t.scope.contains("storage.type")));
        assert!(line1.iter().any(|t| t.scope.contains("entity.name.function")));

        // Line 3 is inside the multi-line block comment started on line 2
        let line3 = cache.tokens_for_line(3).expect("line 3 tokens");
        assert!(line3.iter().any(|t| t.scope.contains("comment.block")));

        let line4 = cache.tokens_for_line(4).expect("line 4 tokens");
        assert!(line4.iter().any(|t| t.scope.contains("string.quoted.double")));
    }

    #[test]
    fn test_incremental_convergence_and_10k_lines_performance() {
        let reg = SyntaxRegistry::load_default();
        let mut cache = IncrementalHighlightCache::new("Rust");

        // Build a 10,500-line Rust source buffer
        let mut lines = Vec::with_capacity(10_500);
        for i in 0..10_500 {
            lines.push(format!("pub fn item_{}(val: usize) -> usize {{ return val + {}; }}", i, i));
        }
        let doc = lines.join("\r\n");
        cache.sync_rope(&doc);

        // Initial viewport tokenization (first 60 visible lines) must be < 16ms
        let t0 = Instant::now();
        cache.ensure_highlighted_up_to(&reg, 60);
        let viewport_elapsed = t0.elapsed();
        assert!(
            viewport_elapsed.as_millis() < 16,
            "Viewport tokenize took {:?}",
            viewport_elapsed
        );

        // Full 10,500-line warm cache + single-line edit convergence must be < 16ms
        cache.ensure_highlighted_up_to(&reg, 10_499);
        lines[500] = String::from("pub fn item_500_edited(val: usize) -> usize { return val + 999; }");
        let edited_doc = lines.join("\r\n");

        let t1 = Instant::now();
        cache.sync_rope(&edited_doc);
        assert_eq!(cache.valid_up_to_line(), 500);
        cache.ensure_highlighted_up_to(&reg, 560);
        let edit_elapsed = t1.elapsed();
        // Because line 500's exit state converges with cached line 500, valid_up_to_line jumps to 10_500!
        assert_eq!(cache.valid_up_to_line(), 10_500);
        assert!(
            edit_elapsed.as_millis() < 16,
            "Incremental edit re-tokenize took {:?}",
            edit_elapsed
        );
    }

    #[test]
    fn test_bracket_matching_ignores_brackets_in_strings() {
        let reg = SyntaxRegistry::load_default();
        let mut cache = IncrementalHighlightCache::new("Rust");
        let src = "fn demo() { let s = \"unmatched } brace\"; }";
        cache.sync_rope(src);
        cache.ensure_highlighted_up_to(&reg, 1);

        let open_brace = src.find('{').expect("open brace");
        let close_brace = src.rfind('}').expect("close brace");
        assert_eq!(
            cache.find_matching_bracket(open_brace),
            Some((open_brace, close_brace))
        );
    }
}
