//! Configuration parsing and pipe-message routing.
//!
//! Lives in the library (not the binary) so the unit tests can exercise
//! these helpers without pulling in the WASI-only `host_run_plugin_command`
//! symbol that the scroll/getter shims depend on.

use std::collections::BTreeMap;

use zellij_tile::prelude::PipeMessage;

use crate::scan::{JumpDir, PromptMatcher};

/// Build the prompt matcher from plugin configuration.
///
/// `prompt_prefixes` is a comma-separated list of literal prefix strings.
/// Whitespace after a comma is stripped (split noise), but whitespace inside
/// an entry is preserved — the trailing space in the default `❯ ` is what
/// keeps "command output that happens to contain `❯`" from being mistaken
/// for a prompt line.
///
/// An empty list (`""`, only commas, or the key missing entirely) falls
/// back to `scan::DEFAULT_PROMPT_PREFIXES`.
pub fn build_matcher(config: &BTreeMap<String, String>) -> PromptMatcher {
    let raw = config
        .get("prompt_prefixes")
        .map(String::as_str)
        .unwrap_or("");
    let prefixes: Vec<String> = raw
        .split(',')
        .map(|s| s.trim_start().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    PromptMatcher::new(prefixes)
}

/// Translate the inbound pipe message into a jump direction.
///
/// Two encodings are accepted so users can pick whichever feels cleaner in
/// the KDL `bind` block:
///
/// * `name = "jump"`, `payload = "next"` (or `prev` / `up` / `down` / …)
/// * `name = "next"` or `name = "prev"` (no payload required)
///
/// Payload wins when both look valid, so a poorly-named pipe is still
/// rescuable by setting a sensible payload.
pub fn direction_from_msg(msg: &PipeMessage) -> Option<JumpDir> {
    if let Some(payload) = msg.payload.as_deref() {
        if let Some(dir) = JumpDir::from_payload(payload) {
            return Some(dir);
        }
    }
    JumpDir::from_payload(&msg.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zellij_tile::prelude::PipeSource;

    fn cfg(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    // ── build_matcher ─────────────────────────────────────────────────────

    #[test]
    fn build_matcher_defaults_when_empty() {
        let m = build_matcher(&BTreeMap::new());
        assert!(m.is_prompt_line("❯ ls"));
        assert!(!m.is_prompt_line("$ ls"));
    }

    #[test]
    fn build_matcher_parses_comma_list() {
        let m = build_matcher(&cfg(&[("prompt_prefixes", "$ , % , > ")]));
        assert!(m.is_prompt_line("$ ls"));
        assert!(m.is_prompt_line("% pwd"));
        assert!(m.is_prompt_line("> hi"));
        assert!(!m.is_prompt_line("❯ ls"));
    }

    #[test]
    fn build_matcher_preserves_trailing_space_in_entry() {
        // The matcher should treat "❯ " (with space) and "❯" differently —
        // the trailing space here is part of the prefix, not split noise.
        let m = build_matcher(&cfg(&[("prompt_prefixes", "❯ ")]));
        assert!(m.is_prompt_line("❯ ls"));
        // "❯ls" (no space) shouldn't match the "❯ " prefix; the default
        // fallback isn't engaged because the explicit list is non-empty.
        assert!(!m.is_prompt_line("❯ls"));
    }

    #[test]
    fn build_matcher_drops_empty_entries() {
        // Stray commas mustn't introduce empty-string prefixes (which would
        // otherwise match every line).
        let m = build_matcher(&cfg(&[("prompt_prefixes", ",$ ,,% ,")]));
        assert!(m.is_prompt_line("$ ls"));
        assert!(m.is_prompt_line("% pwd"));
        assert!(!m.is_prompt_line("plain text"));
    }

    #[test]
    fn build_matcher_missing_key_uses_default() {
        let m = build_matcher(&cfg(&[("unrelated", "x")]));
        assert!(m.is_prompt_line("❯ ls"));
    }

    // ── direction_from_msg ────────────────────────────────────────────────

    fn pipe(name: &str, payload: Option<&str>) -> PipeMessage {
        PipeMessage::new(
            PipeSource::Keybind,
            name,
            &payload.map(String::from),
            &None,
            false,
        )
    }

    #[test]
    fn direction_from_payload_wins() {
        assert_eq!(
            direction_from_msg(&pipe("jump", Some("prev"))),
            Some(JumpDir::Prev)
        );
        assert_eq!(
            direction_from_msg(&pipe("jump", Some("NEXT"))),
            Some(JumpDir::Next)
        );
    }

    #[test]
    fn direction_falls_back_to_name() {
        assert_eq!(direction_from_msg(&pipe("prev", None)), Some(JumpDir::Prev));
        assert_eq!(direction_from_msg(&pipe("next", None)), Some(JumpDir::Next));
    }

    #[test]
    fn direction_name_is_used_when_payload_is_garbage() {
        // Sensible name + nonsense payload → we still resolve via name.
        assert_eq!(
            direction_from_msg(&pipe("next", Some("blarg"))),
            Some(JumpDir::Next)
        );
    }

    #[test]
    fn direction_none_when_unrecognized() {
        assert_eq!(
            direction_from_msg(&pipe("anything", Some("sideways"))),
            None
        );
    }
}
