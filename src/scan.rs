//! Scrollback scanning: find which rows are prompt lines.
//!
//! Zellij's terminal emulator silently drops OSC 133 (semantic prompt
//! markers) because it has no dispatcher for them — the bytes never reach
//! the grid that `get_pane_scrollback` returns. With OSC 133 out of reach,
//! we fall back to a heuristic: a row is a prompt if, after stripping ANSI
//! escapes, it starts with one of a configurable list of prefix strings.
//!
//! This is intentionally simple. The default prefixes target Powerlevel10k's
//! VIINS/VICMD prompt characters (`❯ ` / `❮ `) which sit at column 0 thanks
//! to p10k's `TRANSIENT_PROMPT=always` setting collapsing every past prompt
//! to a single line in scrollback. Users with a different prompt can
//! override the prefix list via plugin config.

use crate::ansi::strip_ansi;

/// Default prefix list when the user does not configure one.
///
/// Order doesn't matter — the matcher returns true on the first hit.
pub const DEFAULT_PROMPT_PREFIXES: &[&str] = &["❯ ", "❮ "];

/// Matcher that decides whether a single scrollback row is a prompt line.
#[derive(Debug, Clone)]
pub struct PromptMatcher {
    prefixes: Vec<String>,
}

impl PromptMatcher {
    /// Build a matcher from an explicit prefix list. An empty `prefixes`
    /// falls back to `DEFAULT_PROMPT_PREFIXES`.
    pub fn new(prefixes: Vec<String>) -> Self {
        let prefixes = if prefixes.is_empty() {
            DEFAULT_PROMPT_PREFIXES
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        } else {
            prefixes
        };
        Self { prefixes }
    }

    /// Default matcher: `❯ ` / `❮ `.
    pub fn default_p10k() -> Self {
        Self::new(Vec::new())
    }

    /// True if the ANSI-stripped `line` starts with any configured prefix.
    pub fn is_prompt_line(&self, line: &str) -> bool {
        let stripped = strip_ansi(line);
        self.prefixes
            .iter()
            .any(|p| stripped.starts_with(p.as_str()))
    }

    /// Scan `lines` and return the indices that match.
    pub fn prompt_rows(&self, lines: &[String]) -> Vec<usize> {
        lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| self.is_prompt_line(l).then_some(i))
            .collect()
    }
}

impl Default for PromptMatcher {
    fn default() -> Self {
        Self::default_p10k()
    }
}

/// Direction of a prompt jump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpDir {
    /// Older prompt — smaller row index.
    Prev,
    /// Newer prompt — larger row index.
    Next,
}

impl JumpDir {
    pub fn from_payload(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "prev" | "previous" | "up" | "back" | "older" => Some(JumpDir::Prev),
            "next" | "down" | "forward" | "newer" => Some(JumpDir::Next),
            _ => None,
        }
    }
}

/// Pick a target row from a sorted list of prompt rows.
///
/// `current_top` is the row index that is currently at the top of the
/// viewport (i.e. the number of lines that exist above the viewport in the
/// scrollback grid).
///
/// * `Prev`: largest `row < current_top` — the most recent prompt that's
///   already offscreen above. Falls back to the topmost known prompt if
///   nothing strictly precedes `current_top` (lets the user keep pressing
///   `p` to walk past the top).
/// * `Next`: smallest `row > current_top` — the closest prompt below the
///   current viewport top. Anchoring at top means re-aligning even an
///   already-visible prompt to row 0.
///
/// Returns `None` if `prompts` is empty or no candidate matches in the
/// requested direction.
pub fn pick_target(prompts: &[usize], current_top: usize, dir: JumpDir) -> Option<usize> {
    if prompts.is_empty() {
        return None;
    }
    match dir {
        JumpDir::Prev => prompts.iter().rev().find(|&&r| r < current_top).copied(),
        JumpDir::Next => prompts.iter().find(|&&r| r > current_top).copied(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    // ── PromptMatcher ──────────────────────────────────────────────────────

    #[test]
    fn default_matcher_uses_p10k_chars() {
        let m = PromptMatcher::default_p10k();
        assert!(m.is_prompt_line("❯ ls -la"));
        assert!(m.is_prompt_line("❮ git status"));
        assert!(!m.is_prompt_line("$ ls"));
        assert!(!m.is_prompt_line("ls -la"));
    }

    #[test]
    fn matcher_strips_sgr_before_check() {
        let m = PromptMatcher::default_p10k();
        assert!(m.is_prompt_line("\x1b[32m❯\x1b[0m hello"));
        assert!(m.is_prompt_line("\x1b[31m❯\x1b[0m \x1b[1mhello\x1b[0m"));
    }

    #[test]
    fn matcher_with_custom_prefixes() {
        let m = PromptMatcher::new(vec!["$ ".into(), "% ".into()]);
        assert!(m.is_prompt_line("$ ls"));
        assert!(m.is_prompt_line("% pwd"));
        assert!(!m.is_prompt_line("❯ ls"));
    }

    #[test]
    fn empty_prefix_list_falls_back_to_default() {
        let m = PromptMatcher::new(Vec::new());
        assert!(m.is_prompt_line("❯ ls"));
    }

    #[test]
    fn matcher_does_not_match_substring() {
        let m = PromptMatcher::default_p10k();
        // The prompt char appears mid-line but isn't a prefix.
        assert!(!m.is_prompt_line("see ❯ here"));
    }

    #[test]
    fn prompt_rows_finds_all_hits() {
        let lines = s(&["❯ ls", "file1", "file2", "❯ cat file1", "contents", "❮ vim"]);
        let m = PromptMatcher::default_p10k();
        assert_eq!(m.prompt_rows(&lines), vec![0, 3, 5]);
    }

    #[test]
    fn prompt_rows_ignores_substring_hits() {
        let lines = s(&["foo ❯ bar", "❯ real prompt"]);
        let m = PromptMatcher::default_p10k();
        assert_eq!(m.prompt_rows(&lines), vec![1]);
    }

    #[test]
    fn prompt_rows_with_ansi_colored_prompt_char() {
        let lines = s(&["\x1b[32m❯\x1b[0m ls", "output", "\x1b[31m❯\x1b[0m grep foo"]);
        let m = PromptMatcher::default_p10k();
        assert_eq!(m.prompt_rows(&lines), vec![0, 2]);
    }

    // ── JumpDir parsing ────────────────────────────────────────────────────

    #[test]
    fn jumpdir_parses_aliases() {
        assert_eq!(JumpDir::from_payload("prev"), Some(JumpDir::Prev));
        assert_eq!(JumpDir::from_payload("PREV"), Some(JumpDir::Prev));
        assert_eq!(JumpDir::from_payload(" up "), Some(JumpDir::Prev));
        assert_eq!(JumpDir::from_payload("previous"), Some(JumpDir::Prev));
        assert_eq!(JumpDir::from_payload("next"), Some(JumpDir::Next));
        assert_eq!(JumpDir::from_payload("DOWN"), Some(JumpDir::Next));
        assert_eq!(JumpDir::from_payload("forward"), Some(JumpDir::Next));
        assert_eq!(JumpDir::from_payload("nope"), None);
        assert_eq!(JumpDir::from_payload(""), None);
    }

    // ── pick_target ────────────────────────────────────────────────────────

    #[test]
    fn pick_target_prev_picks_closest_below() {
        let prompts = vec![2, 5, 12, 30];
        assert_eq!(pick_target(&prompts, 13, JumpDir::Prev), Some(12));
        assert_eq!(pick_target(&prompts, 30, JumpDir::Prev), Some(12));
        assert_eq!(pick_target(&prompts, 100, JumpDir::Prev), Some(30));
    }

    #[test]
    fn pick_target_next_picks_closest_above() {
        let prompts = vec![2, 5, 12, 30];
        assert_eq!(pick_target(&prompts, 0, JumpDir::Next), Some(2));
        assert_eq!(pick_target(&prompts, 5, JumpDir::Next), Some(12));
        assert_eq!(pick_target(&prompts, 12, JumpDir::Next), Some(30));
    }

    #[test]
    fn pick_target_prev_no_candidate_returns_none() {
        let prompts = vec![5, 10];
        assert_eq!(pick_target(&prompts, 5, JumpDir::Prev), None);
        assert_eq!(pick_target(&prompts, 0, JumpDir::Prev), None);
    }

    #[test]
    fn pick_target_next_no_candidate_returns_none() {
        let prompts = vec![5, 10];
        assert_eq!(pick_target(&prompts, 10, JumpDir::Next), None);
        assert_eq!(pick_target(&prompts, 20, JumpDir::Next), None);
    }

    #[test]
    fn pick_target_empty_prompts_returns_none() {
        let prompts: Vec<usize> = Vec::new();
        assert_eq!(pick_target(&prompts, 5, JumpDir::Prev), None);
        assert_eq!(pick_target(&prompts, 5, JumpDir::Next), None);
    }

    #[test]
    fn pick_target_skips_prompt_at_current_top() {
        // A prompt that's exactly at the current viewport top should be
        // skipped — re-pressing `p` should walk past it instead of staying
        // pinned there forever.
        let prompts = vec![5, 10, 15];
        assert_eq!(pick_target(&prompts, 10, JumpDir::Prev), Some(5));
        assert_eq!(pick_target(&prompts, 10, JumpDir::Next), Some(15));
    }
}
