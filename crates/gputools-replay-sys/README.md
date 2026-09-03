# gputools-replay-sys

Raw FFI bindings to Apple's **private** `GPUToolsReplay` framework, new in
macOS 27. There are no headers or documentation for this framework; every
signature and layout fact in this crate was read from disassembly or from
the live Objective-C runtime, and each is validated on exactly one machine.
See the workspace [root README](../../README.md) for the wider picture and
`docs/HANDOFF.md` for the full reverse-engineering record this crate is
built from.

This crate holds only what the framework itself dictates: link
configuration, established FFI signatures, the replay client's memory
layout with a regression test, and an inventory of the exported surface.
It carries no policy. It does not validate capture bundles, manage a
session's lifecycle, pump a run loop, or set environment variables. Those
belong to a safe layer built on top (`crates/gputools-replay`, currently a
stub) or to campaign probes.

## Requirements: macOS 27 or newer

`build.rs` links `GPUToolsReplay` from `/System/Library/PrivateFrameworks`,
resolved by the linker from the running system's dyld shared cache at build
time. There is no `.tbd` stub on that path and no `dlopen`. This means:

- The build only works on macOS. It fails loudly, with a clear message, on
  any other `target_os`.
- The build refuses to proceed on a host older than macOS 27. The
  framework's ABI here was read from disassembly on macOS 27, and the
  macOS 26 SDK's stub is missing at least one export
  (`GTReplayFetchAccelerationStructure`) that this crate's inventory
  tracks. Building against an older host would silently target a
  different, unverified ABI.
- Cross-compiling is not supported: this is an OS-installed private
  framework, so the build inherently targets whatever OS it runs on.

## The unlock: verify, don't set

`lockParameterBufferSizeToMax` defaults to `1`, and with it set, the
replay device's command-queue creation returns nil in an unentitled
process, so every fetch fails. The fix is one environment variable:

```
MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0
```

This crate never sets that variable. `env::unlock_env_ok()` and
`env::check_unlock_env()` only **verify** it is set to the literal string
`"0"` and return a named error if not. The reason is soundness, not
policy: `std::env::set_var` is `unsafe` in edition 2024 and sound only
while the process is single-threaded, a precondition only a binary's
`main` can guarantee, never a library. A caller of this crate is
responsible for setting the variable as the first statement of its `main`,
before spawning any thread, and this crate's job is to refuse to proceed
silently if that was not done.

## The client buffer: a derived 312-byte layout

The framework's bootstrap sequence hands `GTMTLReplayClient_init` a
caller-allocated buffer for an opaque struct it does not describe in any
header. This crate supplies `client::ClientBuffer`, a `#[repr(C,
align(16))]` wrapper over `[u8; 312]`.

That size was not measured by trial and error; it was derived and then
checked:

1. `-[GTMTLReplayService initWithContext:]`'s Objective-C runtime type
   encoding contains the struct's complete layout. This crate reads it
   from the **live runtime** (`class_getInstanceMethod` +
   `method_getTypeEncoding`), never from a transcription in prose. An
   earlier hand-copy of this same encoding, made for the prior project
   this one builds on, truncated it by 176 bytes.
2. `NSGetSizeAndAlignment` cannot parse the encoding; it raises an
   `NSInvalidArgumentException` on the bitfield run inside it. So the
   layout was rebuilt as an equivalent C struct, its `@encode()` was
   checked byte-for-byte against the runtime's string, and `sizeof` was
   taken from that: 312 bytes, alignment 8.
3. This was then cross-checked live: a large buffer was poisoned outside
   the derived extent and the framework's real bootstrap-and-fetch
   lifecycle was run against it. Every write landed within `[0x00,
   0x12c]`; nothing at or past `0x138` was touched.

Two things the encoding cannot settle, documented in full in
`src/client.rs`: a bitfield's declared storage type (checked, and shown
harmless across all plausible choices) and whether any member carries an
`aligned` or `packed` attribute the encoding string would not reveal
(unresolved in principle, but constrained by the write-extent measurement
landing exactly where the derived layout predicts).

The regression test (`client::tests::the_encoding_the_client_size_was_derived_from_is_unchanged`)
re-reads the encoding from the live runtime on every `cargo test` and
fails on any difference from the recorded copy. This is the crate's
publication gate: it turns a macOS update that changes this struct's
layout into a build failure instead of memory corruption, and it must
never be removed, skipped, or feature-gated.

## Coverage: established versus unverified

The framework exports 31 symbols in total (18 C functions, 13 Objective-C
classes), read from `GPUToolsReplay.tbd` in the macOS 27 SDK.
`inventory::EXPORTS` lists every one of them with a status:

- **Established**: a signature or class binding in this crate is
  probe-confirmed, and is declared as a callable `extern "C"` function or
  an objc2 class binding.
- **Unverified**: the symbol exists and is recorded, but this crate does
  not declare it as callable. A guessed signature against a private,
  undocumented framework is undefined behavior waiting for a call site;
  this crate would rather be visibly incomplete than silently unsound.

`inventory::coverage()` reports `(established, total)` over that list, and
a test pins it to the current state (5 of 31 as of this writing) so the
count cannot drift silently out of sync with `EXPORTS`. As the
reverse-engineering campaign in `docs/findings/` confirms more surfaces,
symbols graduate from unverified to established, each with a doc comment
recording how it was confirmed. A new signature is never declared callable
here until a probe has confirmed it; see `docs/findings/` for what has and
has not been probed yet.
