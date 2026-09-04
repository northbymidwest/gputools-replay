//! Raw bindings to Apple's private `GPUToolsReplay` framework, the engine
//! behind Xcode's GPU debugger and the `gpudebug` CLI. macOS 27+ only.
//!
//! This crate holds only what the framework itself dictates: link
//! configuration, established FFI signatures, the replay client's memory
//! layout with a regression test that re-derives it from the live ObjC
//! runtime, and an inventory of the exported surface. Policy (bundle
//! validation, session lifecycle, error handling) lives in higher layers.
//!
//! Provenance: every signature here was read from disassembly or the live
//! runtime, never from a header, because none exist. Each item's doc comment
//! states how it was established. See `docs/HANDOFF.md` in the repository.

pub mod client;
pub mod env;
pub mod ffi;
pub mod inventory;
pub mod replay;
