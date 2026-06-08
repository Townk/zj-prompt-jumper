//! zj-prompt-jumper — jump to previous/next shell prompt in scroll mode.
//!
//! Loaded headless at session start via `load_plugins`. User keybindings in
//! Zellij's `scroll` mode (default `p` / `n`) dispatch into the plugin via
//! `MessagePlugin`; on each message the plugin:
//!
//! 1. resolves the focused terminal pane,
//! 2. snapshots its scrollback (lines above viewport + viewport + lines
//!    below) via the synchronous `get_pane_scrollback` shim,
//! 3. scans those rows for prompt-line prefixes (default: `❯ ` / `❮ ` —
//!    Powerlevel10k's VIINS/VICMD chars), and
//! 4. issues the right number of `scroll_up_in_pane_id` /
//!    `scroll_down_in_pane_id` calls to put the target prompt at the top
//!    of the viewport.
//!
//! ## Why not OSC 133?
//!
//! Zellij's VTE parser silently drops OSC 133 markers (it only dispatches
//! OSC 7 today), so the markers never reach the grid the plugin can query.
//! Until upstream gains OSC 133 support we match on the printable prompt
//! prefix instead. See `scan.rs` for the matcher.
//!
//! ## Configuration
//!
//! Plugin config (set in the `plugin { … }` block where the plugin is
//! aliased, or in the launching `MessagePlugin { … }`):
//!
//! * `prompt_prefixes` — comma-separated list of prefix strings. Defaults
//!   to `❯ ,❮ `. Each line that, after stripping ANSI, starts with one of
//!   these is treated as a prompt.

use std::collections::BTreeMap;

use zellij_tile::prelude::*;

use zj_prompt_jumper::config::{build_matcher, direction_from_msg};
use zj_prompt_jumper::scan::{pick_target, PromptMatcher};

register_plugin!(State);

/// Hard cap on how many single-line scroll calls we'll issue per jump.
/// Each call is a one-way host command, so the per-call cost is small,
/// but absurd jumps (a 50k-line scrollback) shouldn't pin the plugin
/// thread either. With a 10k default scrollback this is comfortable.
const MAX_SCROLL_STEPS: usize = 50_000;

/// Upper bound on pipe messages buffered while the permission grant is still
/// pending. The grant arrives shortly after `load`, so only the very first
/// chord or two should ever queue; cap it so a stuck/slow grant can't let the
/// buffer grow without limit. We keep the most recent messages.
const MAX_PENDING_PIPES: usize = 8;

/// Lifecycle of the plugin's permission request.
///
/// The grant arrives asynchronously after `load`, so we start `Pending` and
/// buffer early pipe messages. A `Denied` result is terminal: we stop
/// buffering and drop incoming messages instead of growing an unbounded queue
/// that can never be drained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PermissionState {
    #[default]
    Pending,
    Granted,
    Denied,
}

#[derive(Default)]
struct State {
    permissions: PermissionState,
    pending_pipes: Vec<PipeMessage>,
    matcher: PromptMatcher,
}

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.matcher = build_matcher(&configuration);

        request_permission(&[
            // get_focused_pane_info / get_pane_scrollback.
            PermissionType::ReadApplicationState,
            // scroll_up_in_pane_id / scroll_down_in_pane_id.
            PermissionType::ChangeApplicationState,
            // get_pane_scrollback returns grid contents.
            PermissionType::ReadPaneContents,
        ]);
        subscribe(&[EventType::PermissionRequestResult]);

        // Intentionally NOT calling `set_selectable(false)` here. When the
        // user hasn't yet approved the plugin's permissions, Zellij overlays
        // a prompt on the plugin's pane and the user needs to be able to
        // focus that pane to accept. Marking the pane non-selectable in
        // `load` makes the prompt unreachable forever — the plugin sits
        // pending, every pipe message gets buffered, and `p`/`n` in scroll
        // mode silently does nothing. We defer the non-selectable flip to
        // `update` after the grant arrives.
    }

    fn update(&mut self, event: Event) -> bool {
        if let Event::PermissionRequestResult(status) = event {
            if status == PermissionStatus::Granted {
                self.permissions = PermissionState::Granted;
                // Permissions are in hand; hide the pane from focus / nav
                // so the headless plugin stops eating Tab cycles.
                set_selectable(false);
                let pending: Vec<PipeMessage> = self.pending_pipes.drain(..).collect();
                for msg in pending {
                    self.handle_pipe(msg);
                }
            } else {
                // Denial is terminal: drop the buffer and stop queuing so the
                // pending list can't grow for the rest of the session.
                self.permissions = PermissionState::Denied;
                self.pending_pipes.clear();
                self.pending_pipes.shrink_to_fit();
                eprintln!(
                    "zj-prompt-jumper: permission request denied; \
                     plugin will not respond to scroll-mode jumps"
                );
            }
        }
        false
    }

    fn pipe(&mut self, msg: PipeMessage) -> bool {
        match self.permissions {
            // Grant arrives asynchronously after `load`; buffer early pipe
            // messages so the very first `p` / `n` chord after session start
            // isn't silently dropped. Cap the buffer and keep the most recent
            // messages so a stuck grant can't grow it without bound.
            PermissionState::Pending => {
                if self.pending_pipes.len() >= MAX_PENDING_PIPES {
                    self.pending_pipes.remove(0);
                }
                self.pending_pipes.push(msg);
            }
            PermissionState::Granted => self.handle_pipe(msg),
            // Permanently denied: nothing can handle the message, so drop it.
            PermissionState::Denied => {}
        }
        false
    }

    fn render(&mut self, _rows: usize, _cols: usize) {}
}

impl State {
    fn handle_pipe(&mut self, msg: PipeMessage) {
        let Some(dir) = direction_from_msg(&msg) else {
            // Not a jump command. Plugin-to-plugin broadcasts (e.g. zj-hud's
            // `__zj_hud_sync_state`) are sprayed to every loaded plugin and are
            // none of our business. Never log the payload: those blobs are
            // multi-KB of escaped UTF-8 and dumping them overflows Zellij's
            // per-plugin stderr buffer, which traps the plugin and kills
            // jumping entirely. Only surface a name-only diagnostic for
            // keybind/CLI sources, where an unrecognized name is a real
            // misconfiguration worth seeing.
            if !matches!(msg.source, PipeSource::Plugin(_)) {
                eprintln!("zj-prompt-jumper: unrecognized direction (name='{}')", msg.name);
            }
            return;
        };

        let pane_id = match get_focused_pane_info() {
            Ok((_tab, PaneId::Terminal(id))) => PaneId::Terminal(id),
            // Focused pane is a plugin (e.g. session-manager floating on
            // top); nothing to scroll.
            Ok(_) => return,
            Err(_) => return,
        };

        let contents = match get_pane_scrollback(pane_id, true) {
            Ok(c) => c,
            Err(err) => {
                eprintln!("zj-prompt-jumper: get_pane_scrollback failed: {err}");
                return;
            }
        };

        let current_top = contents.lines_above_viewport.len();
        let mut all_lines: Vec<String> = Vec::with_capacity(
            contents.lines_above_viewport.len()
                + contents.viewport.len()
                + contents.lines_below_viewport.len(),
        );
        all_lines.extend(contents.lines_above_viewport);
        all_lines.extend(contents.viewport);
        all_lines.extend(contents.lines_below_viewport);

        let prompts = self.matcher.prompt_rows(&all_lines);
        let Some(target) = pick_target(&prompts, current_top, dir) else {
            // No candidate in the requested direction; nothing to do.
            return;
        };

        apply_scroll(pane_id, current_top, target);
    }
}

/// Issue the right number of single-line scrolls to land `target` at the
/// top of the viewport.
///
/// `scroll_up_in_pane_id` moves the viewport upward (older content), which
/// means the row currently at the top gets a *smaller* index in the next
/// snapshot. `scroll_down_in_pane_id` is the mirror image. So a positive
/// `delta` (target below current top) needs `scroll_down` calls, and a
/// negative one needs `scroll_up`.
fn apply_scroll(pane_id: PaneId, current_top: usize, target: usize) {
    if target >= current_top {
        let n = (target - current_top).min(MAX_SCROLL_STEPS);
        for _ in 0..n {
            scroll_down_in_pane_id(pane_id);
        }
    } else {
        let n = (current_top - target).min(MAX_SCROLL_STEPS);
        for _ in 0..n {
            scroll_up_in_pane_id(pane_id);
        }
    }
}
