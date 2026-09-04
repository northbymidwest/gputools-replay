# gputools-replay

[![github](https://img.shields.io/badge/github-northbymidwest%2Fgputools--replay-blue?logo=github)](https://github.com/northbymidwest/gputools-replay)
[![crates.io](https://img.shields.io/crates/v/gputools-replay.svg)](https://crates.io/crates/gputools-replay)
[![docs.rs](https://docs.rs/gputools-replay/badge.svg)](https://docs.rs/gputools-replay)
[![CI](https://github.com/northbymidwest/gputools-replay/actions/workflows/ci.yml/badge.svg)](https://github.com/northbymidwest/gputools-replay/actions/workflows/ci.yml)

A Rust interface to `GPUToolsReplay`, a **private** Apple framework new in
macOS 27. It is the engine behind Xcode's GPU debugger and the `gpudebug`
CLI, and it lets a `.gputrace` capture be driven programmatically: loaded,
replayed, and queried for the textures, pipelines, and other resources it
contains.

> [!WARNING]
> **This links a private, undocumented Apple framework. It can break at any
> time.**
>
> `GPUToolsReplay` ships no headers, no documentation, and no stability
> guarantee. Everything here is reverse-engineered from disassembly and the
> live runtime, so two things can break it, both silently:
>
> - **Errors in the reverse engineering.** A struct layout, field offset, or
>   method signature that was read wrong is undefined behavior against a
>   private framework - wrong results or a crash, not a compile error.
> - **Apple changing it out from under us.** Apple can alter or remove the
>   framework, or change its ABI or the `.gputrace` format, in any macOS
>   update, with no notice and no deprecation window.
>
> Treat it as experimental: pin an exact version, expect it to need
> re-validation on every macOS update, and do not put anything load-bearing on
> it without checks of your own.

There are no headers and no documentation for this framework. Everything
this workspace knows about it was read from disassembly and the live
Objective-C runtime, then checked by running real probes against a real
capture. It has been validated on exactly one machine, running exactly one
version of macOS 27. Read that as a caution, not a disclaimer: treat any
claim in this repository the way its own findings are written, as something
measured, with the method stated, not as something to be trusted on
authority.

## What is here

A reverse-engineered, pre-release (`0.x`) stack of crates over the framework,
plus the campaign's evidence trail:

- **`crates/gputools-replay-sys`** is the FFI layer: link configuration, the
  bootstrap function signatures, the replay client's memory layout, and a
  regression test that re-derives that layout from the live runtime on every
  `cargo test`. See [its README](crates/gputools-replay-sys/README.md).
- **`crates/gputools-replay`** is the safe, in-process wrapper: a
  one-session-per-process model, texture / buffer / heap / pipeline-binaries /
  acceleration-structure fetch, playback, and harvester decoding, with typed
  results that preserve every field. Only `Session::configure_env` is `unsafe`
  (an opt-in for non-default replayer config); every other public fn is safe.
- **`crates/gputools-replay-hl`** is the ergonomic domain layer over it:
  format-aware textures, typed buffers, and the descriptor join that reconciles
  fetched resources against the capture's on-disk manifest.
- **`probes/`** is an unpublished workspace member, one Rust binary per live or
  static probe. Campaign tooling, not examples, and not meant to be depended on.
- **`docs/findings/`** and **`docs/HANDOFF.md`** are the campaign's evidence
  log: the measured facts, each with the method that established it. Start there
  to see what is established versus still guessed.

The portable, framework-free `.gputrace` bundle reader lives in its own
repository,
[`gputrace-bundle`](https://github.com/northbymidwest/gputrace-bundle).

## macOS version

The ABI was reverse-engineered on macOS 27, so the default `macos27` feature
floors the build there. Building `--no-default-features` lowers the floor to
macOS 26, which ships the same framework minus the
`GTReplayFetchAccelerationStructure` class: acceleration-structure fetch returns
an error there, and nothing else changes.

## Operational constraints

The replayer is a shared, stateful, and crash-prone system resource.
Anything that touches it, including the probes in this workspace, must
respect the following (see `docs/HANDOFF.md` section 4 for the full
account):

- **Access must be serialized.** Only one replay session runs at a time,
  machine-wide.
- **One session per process.** A second bootstrap in the same process
  aborts with exit code 132.
- **An interrupted run orphans the session and locks the replayer for TWO
  HOURS.** Do not interrupt a run on impatience alone; latency legitimately
  ranges from about 27 seconds to over 20 minutes.
- **Recovery**, if a session is stuck:

  ```
  gpudebug --terminate all
  pkill -9 -f GPUToolsReplayService
  ```

- **Check `pgrep -f GPUToolsReplayService` before and after every run.** It
  should print nothing in the clean state. `probes/run.sh` wraps this
  hygiene automatically for probe binaries.

## The encoding regression test

`crates/gputools-replay-sys` hands the framework a caller-allocated,
312-byte buffer for its internal client structure. That size was derived
from the Objective-C runtime's own type encoding of a live method, not
guessed or copied from a document. A test in that crate re-reads the
encoding from the live runtime on every `cargo test` and fails on any
byte-level difference from the recorded copy.

This test is what makes the whole approach defensible rather than
reckless: it turns a macOS update that changes the framework's internal
layout into a loud build failure instead of silent heap corruption. **It
must never be removed, skipped, or feature-gated.**

## Sanity-checking the substrate

`probes/run.sh smoke` runs the `smoke` probe: bootstrap the framework,
load `captures/small.gputrace`, sweep streamRefs at natural size, and
parse the reply. This has been run end-to-end against the real framework
on macOS 27.0 and returned 4 `BGRA8Unorm` records, matching the capture's
known-good subset documented in `captures/README.md`. That is the
fastest way to confirm the substrate still works on a given machine
before trusting anything built on top of it.

Captures themselves are not committed (they run tens of megabytes each);
`captures/README.md` documents what each one contains and how to obtain
it.

## Working on this repo

One setup step per clone, to enable the tracked git hooks:

```
git config core.hooksPath .githooks
```

`.githooks/pre-commit` rejects any commit that leaves the workspace failing
`cargo fmt --all --check`, and names the offending files. Fix with
`cargo fmt --all`; `git commit --no-verify` bypasses it for a genuine
emergency. The hook no-ops outside a Rust repo and when rustfmt is not
installed, so it is safe to copy elsewhere.

## Requirements

macOS 27 or newer, and nothing else. This is an OS-installed private
framework resolved from the running system's dyld shared cache at build
time; there is no supported way to cross-compile against it or to target
an older macOS.

## Further reading

- `docs/HANDOFF.md`, the distilled result of the reverse-engineering work
  behind this workspace: established facts with their method, open
  questions, and the operational constraints above, in full.
- `docs/findings/`, the campaign's per-surface evidence, updated as probes
  run.

## License

[BSD Zero Clause License](LICENSE)

### Why 0BSD?

The majority of this codebase was generated by AI coding agents (primarily
Claude). AI-generated code is not copyrightable and is effectively public
domain, making 0BSD, which imposes no restrictions on use, the most
appropriate license.

### Disclaimer

While AI-generated code itself is public domain, AI agents may have reproduced
or closely derived code from copyrighted sources (training data, reference
implementations, open-source projects, etc.). No audit has been conducted to
identify such instances, as this is a personal side project. Any such code
fragments remain subject to the licenses of their original creators. Use at
your own discretion.
