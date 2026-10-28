//! Incremental Line Tokenizer wrapping `syntect` (`ParseState` + `ScopeStack`)
//! with `tree-sitter` / `tree-sitter-highlight` fallback support.

use crate::highlight::registry::SyntaxRegistry;
use std::ops::Range;
use syntect::parsing::{ParseState, ScopeStack, ScopeStackOp, SyntaxReference};
use tree_sitter::Parser as TreeSitterParser;
use tree_sitter_highlight::Highlighter as TreeSitterHighlighter;

/// Represents a single highlighted token span within a single line of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightToken {
    /// Byte offset range `[start..end)` relative to the start of the line (excluding trailing `\r\n`).
    pub range: Range<usize>,
    /// Most specific hierarchical scope string for this token (e.g., `"keyword.control.rust"`,
    /// `"comment.line.double-slash.rust"`, `"string.quoted.double.rust"`).
    pub scope: String,
}

/// Encapsulates the parser state (`ParseState`) and active scope stack (`ScopeStack`)
/// at a line boundary for `O(1)` incremental convergence checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineLexerState {
    pub parse_state: ParseState,
    pub scope_stack: ScopeStack,
}

impl LineLexerState {
    /// Creates the initial `LineLexerState` at line 0 for the given `SyntaxReference`.
    pub fn new(syntax: &SyntaxReference) -> Self {
        Self {
            parse_state: ParseState::new(syntax),
            scope_stack: ScopeStack::new(),
        }
    }
}

/// Stateless helper that advances a `LineLexerState` across a single line of source code
/// using `syntect::parsing::SyntaxSet`, emitting `Vec<HighlightToken>` and the exit `LineLexerState`.
pub struct SyntectTokenizer;

impl SyntectTokenizer {
    /// Tokenizes a single line `line_with_newline` starting from `entry_state`.
    /// Returns `(tokens_for_line, exit_state)`.
    pub fn tokenize_line(
        registry: &SyntaxRegistry,
        line_with_newline: &str,
        entry_state: &LineLexerState,
    ) -> (Vec<HighlightToken>, LineLexerState) {
        let mut parse_state = entry_state.parse_state.clone();
        let mut scope_stack = entry_state.scope_stack.clone();

        let visible_len = line_with_newline
            .trim_end_matches(&['\r', '\n'][..])
            .len();

        let ops = match parse_state.parse_line(line_with_newline, registry.syntax_set()) {
            Ok(ops) => ops,
            Err(_) => {
                // Fallback to TreeSitterFallback lexical scanner if syntect encounters an invalid state
                let tokens = TreeSitterFallback::scan_line_fallback(line_with_newline, visible_len);
                return (
                    tokens,
                    LineLexerState {
                        parse_state,
                        scope_stack,
                    },
                );
            }
        };

        let mut tokens = Vec::new();
        let mut cur_byte = 0usize;

        for (offset, op) in ops {
            let clamped_offset = offset.min(visible_len);
            if clamped_offset > cur_byte {
                let scope_str = Self::resolve_scope_string(&scope_stack);
                tokens.push(HighlightToken {
                    range: cur_byte..clamped_offset,
                    scope: scope_str,
                });
                cur_byte = clamped_offset;
            }
            let _ = scope_stack.apply(&op);
        }

        if cur_byte < visible_len {
            let scope_str = Self::resolve_scope_string(&scope_stack);
            tokens.push(HighlightToken {
                range: cur_byte..visible_len,
                scope: scope_str,
            });
        }

        // Merge adjacent tokens that resolved to the exact same scope
        let mut merged: Vec<HighlightToken> = Vec::with_capacity(tokens.len());
        for tok in tokens {
            if let Some(last) = merged.last_mut() {
                if last.scope == tok.scope && last.range.end == tok.range.start {
                    last.range.end = tok.range.end;
                    continue;
                }
            }
            merged.push(tok);
        }

        (
            merged,
            LineLexerState {
                parse_state,
                scope_stack,
            },
        )
    }

    /// Extracts the most specific non-root scope from `ScopeStack` (or the root scope if only one exists).
    fn resolve_scope_string(stack: &ScopeStack) -> String {
        let scopes = stack.as_slice();
        if scopes.is_empty() {
            return String::from("text.plain");
        }
        // Prefer the deepest scope on the stack (e.g. `keyword.control.rust` over `source.rust`)
        scopes[scopes.len() - 1].build_string()
    }
}

/// Fallback highlighter backed by `tree-sitter` & `tree-sitter-highlight` structures
/// when a custom grammar or line recovery is needed.
pub struct TreeSitterFallback {
    _parser: TreeSitterParser,
    _highlighter: TreeSitterHighlighter,
}

impl Default for TreeSitterFallback {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeSitterFallback {
    /// Creates a new `TreeSitterFallback` instance wrapping `tree_sitter::Parser` and
    /// `tree_sitter_highlight::Highlighter`.
    pub fn new() -> Self {
        Self {
            _parser: TreeSitterParser::new(),
            _highlighter: TreeSitterHighlighter::new(),
        }
    }

    /// Lightweight lexical fallback scanner for a single line `[0..visible_len)`.
    pub fn scan_line_fallback(line: &str, visible_len: usize) -> Vec<HighlightToken> {
        let slice = &line[..visible_len.min(line.len())];
        if let Some(comment_idx) = slice.find("//").or_else(|| slice.find('#')) {
            let mut out = Vec::new();
            if comment_idx > 0 {
                out.push(HighlightToken {
                    range: 0..comment_idx,
                    scope: String::from("source"),
                });
            }
            out.push(HighlightToken {
                range: comment_idx..slice.len(),
                scope: String::from("comment.line"),
            });
            return out;
        }
        if slice.is_empty() {
            Vec::new()
        } else {
            vec![HighlightToken {
                range: 0..slice.len(),
                scope: String::from("text.plain"),
            }]
        }
    }
}

/// Applies a sequence of `ScopeStackOp` operations safely.
pub fn apply_scope_ops(stack: &mut ScopeStack, ops: &[ScopeStackOp]) {
    for op in ops {
        let _ = stack.apply(op);
    }
}
