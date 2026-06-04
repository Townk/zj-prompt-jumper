# zj-prompt-jumper

[![CI](https://github.com/Townk/zj-prompt-jumper/actions/workflows/ci.yml/badge.svg)](https://github.com/Townk/zj-prompt-jumper/actions/workflows/ci.yml)
[![Latest build](https://img.shields.io/github/v/release/Townk/zj-prompt-jumper?include_prereleases&label=latest)](https://github.com/Townk/zj-prompt-jumper/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A tiny [Zellij](https://zellij.dev) plugin that jumps the scrollback to the
**previous / next shell prompt** while you're in scroll mode — like `[[` / `]]`
for your terminal history. Press a key and the viewport snaps so the adjacent
prompt sits at the top, instead of scrolling line-by-line to find where the
last command started.

The plugin runs headless: you load it once at session start and dispatch into
it from your scroll-mode keybindings via `MessagePlugin`.

## How it works

1. On each keypress, the plugin resolves the focused terminal pane.
2. It snapshots that pane's scrollback (lines above the viewport + the
   viewport + lines below) via Zellij's synchronous `get_pane_scrollback`.
3. It scans those rows for **prompt lines** — rows that, after ANSI escapes
   are stripped, start with one of a configurable list of prefixes (default:
   `❯ ` / `❮ `, Powerlevel10k's VIINS/VICMD characters).
4. It picks the nearest prompt in the requested direction and issues the right
   number of single-line scrolls to land it at the top of the viewport.

### Why match on a prefix instead of OSC 133?

The semantic-prompt escape (OSC 133) would be the "correct" signal, but
Zellij's terminal parser currently drops OSC 133 markers (it only dispatches
OSC 7), so they never reach the grid a plugin can query. Until upstream gains
OSC 133 support, matching the printable prompt prefix is the reliable option.

## Install

Every release ships a prebuilt `zj-prompt-jumper.wasm` — you don't need a Rust
toolchain to use the plugin. Two stable download URLs are published:

| URL | Tracks |
|---|---|
| `https://github.com/Townk/zj-prompt-jumper/releases/download/latest/zj-prompt-jumper.wasm` | The **rolling build** — refreshed on every push to `master`. |
| `https://github.com/Townk/zj-prompt-jumper/releases/download/v0.1.0/zj-prompt-jumper.wasm` | A **pinned version** — immutable once published. |

### Option A — reference the release URL directly (recommended)

Zellij can load a plugin straight from a URL and caches it locally, so you can
point an alias at the release asset and skip the manual download. Define the
alias once in the `plugins` block of `~/.config/zellij/config.kdl`:

```kdl
plugins {
    // ...keep Zellij's default aliases here...
    prompt-jumper location="https://github.com/Townk/zj-prompt-jumper/releases/download/latest/zj-prompt-jumper.wasm"
}
```

> Pin to a `v*` URL for reproducible setups; use the `latest` URL to always
> ride the newest build. After changing the URL, clear Zellij's plugin cache
> (`~/.cache/zellij`) or restart the session to force a re-download.

### Option B — download the asset into your plugins directory

```sh
mkdir -p ~/.config/zellij/plugins
curl -fL -o ~/.config/zellij/plugins/zj-prompt-jumper.wasm \
  https://github.com/Townk/zj-prompt-jumper/releases/download/latest/zj-prompt-jumper.wasm
```

Then point the alias at the local file instead:

```kdl
plugins {
    prompt-jumper location="file:~/.config/zellij/plugins/zj-prompt-jumper.wasm"
}
```

`just install` does the build-and-copy in one step if you're building from
source.

## Configure

With the `prompt-jumper` alias defined (see above), load it at session start
and bind keys in **scroll mode**:

```kdl
// Load the headless plugin once at session start.
load_plugins {
    prompt-jumper
}

keybinds {
    scroll {
        // Jump to the previous (older) prompt.
        bind "p" {
            MessagePlugin "prompt-jumper" {
                name "prev"
            }
        }
        // Jump to the next (newer) prompt.
        bind "n" {
            MessagePlugin "prompt-jumper" {
                name "next"
            }
        }
    }
}
```

Enter scroll mode (default `Ctrl b`) and tap `p` / `n` to hop between prompts.
The bindings stay in scroll mode, so you can keep pressing to walk the history.

> On first use Zellij asks you to approve the plugin's permissions
> (`ReadApplicationState`, `ChangeApplicationState`, `ReadPaneContents`).
> Approve once and it stays granted for the session.

### Choosing a direction

The plugin accepts two encodings, so you can pick whichever reads better in
your config:

- `name "prev"` / `name "next"` — no payload required (used above).
- `name "jump"` with `payload "prev"` / `payload "next"` — handy if you'd
  rather keep a single message name.

Each direction has aliases (case-insensitive): `prev` also accepts
`previous` / `up` / `back` / `older`; `next` also accepts `down` / `forward` /
`newer`. When both `name` and `payload` are present, a valid `payload` wins.

### Custom prompt prefixes

If your prompt doesn't start with `❯ ` / `❮ `, override the prefixes in the
alias definition. `prompt_prefixes` is a comma-separated list of literal
prefix strings; whitespace **inside** an entry is significant (the trailing
space in `❯ ` is what stops command output containing `❯` from being mistaken
for a prompt):

```kdl
plugins {
    prompt-jumper location="https://github.com/Townk/zj-prompt-jumper/releases/download/latest/zj-prompt-jumper.wasm" {
        // e.g. a plain "$ " / "% " prompt:
        prompt_prefixes "$ ,% "
    }
}
```

An empty or missing list falls back to the `❯ ` / `❮ ` defaults.

> **Tip (Powerlevel10k):** the matcher works best when each past prompt is a
> single line pinned at column 0. p10k's `TRANSIENT_PROMPT` collapses previous
> prompts to exactly that, which is what makes prefix matching reliable.

## Build from source

```sh
rustup target add wasm32-wasip1   # one-time
cargo build --release --target wasm32-wasip1
```

The artifact is `target/wasm32-wasip1/release/zj-prompt-jumper.wasm`. Copy it
to wherever you keep Zellij plugins, or run `just install`.

## Development

The host-agnostic logic (ANSI stripping, prompt scanning, direction parsing)
lives in the library crate and is unit-tested on the host toolchain — no WASM
runtime required:

```sh
cargo test                 # run the unit tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

A [`justfile`](justfile) wraps the common tasks (`just build`, `just test`,
`just lint`, `just install`, `just reload`). CI runs the same lint/test/build
matrix on every push, and pushing a `vX.Y.Z` tag publishes a versioned
release.

## Limitations

- Prompt detection is a **prefix heuristic**, not semantic: a row only counts
  as a prompt if it starts with a configured prefix after ANSI stripping. A
  mid-line `❯` won't match.
- Best results need each past prompt to occupy a single line at column 0 (see
  the Powerlevel10k tip above). Multi-line prompts only match on the row that
  carries the configured prefix.
- Only the focused **terminal** pane is scanned; plugin/floating panes are
  ignored.
- A single jump is capped at 50,000 single-line scroll steps, comfortably
  above the default 10k scrollback.

## License

Licensed under the [MIT License](LICENSE).
