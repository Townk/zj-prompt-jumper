//! Host-agnostic logic for zj-prompt-jumper.
//!
//! The plugin binary (`src/main.rs`) wires this into the Zellij plugin
//! runtime, but the prompt-scanning logic lives here so it can be unit-tested
//! on the host toolchain without dragging in the WASI-only `zellij-tile`
//! shim symbols.

pub mod ansi;
pub mod config;
pub mod scan;
