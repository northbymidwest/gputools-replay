# gputools-replay

A Rust interface to `GPUToolsReplay`, a **private** Apple framework new in
macOS 27. It is the engine behind Xcode's GPU debugger and the `gpudebug`
CLI, and it lets a `.gputrace` capture be driven programmatically: loaded,
replayed, and queried for the textures, pipelines, and other resources it
contains.

There are no headers and no documentation for this framework. Everything
this workspace knows about it was read from disassembly and the live
Objective-C runtime, then checked by running real probes against a real
capture. It has been validated on exactly one machine, running exactly one
version of macOS 27. Read that as a caution, not a disclaimer: treat any
claim in this repository the way its own findings are written, as something
measured, with the method stated, not as something to be trusted on
authority.

## What is here, and what is not

This is a reverse-engineering campaign with a Cargo workspace attached, not
a finished library. Concretely:

- **`crates/gputools-replay-sys`** is the FFI layer: link configuration,
  the bootstrap function signatures, the replay client's memory layout, and
  a regression test that re-derives that layout from the live runtime on
  every `cargo test`. This is the crate intended for eventual publication.
  See [its README](crates/gputools-replay-sys/README.md) for the details.
- **`crates/gputools-replay`** is a stub. The name is reserved and the
  crate builds, but it has no API. The safe, high-level interface (a
  session model, query and result types, error handling) is deliberately
  **not implemented yet**, it is designed in a second pass, after the
  reverse-engineering campaign has established how the framework's unknown
  surfaces behave. Do not expect a working high-level API from this crate
  today.
- **`probes/`** is an unpublished workspace member holding one Rust binary
  per live or static probe against the framework. These are campaign
  tooling, not examples, and are not meant to be depended on.
- **`docs/findings/`** is the campaign's evidence log: one dossier per
  surface of the framework's exported API, each recording an expectation
  written before a probe ran and the result recorded after, including
  nulls and surprises. Start there if you want to know what is actually
  established versus still guessed.

## Why the safe crate is not implemented

The framework exports 31 symbols (18 C functions, 13 Objective-C classes).
Only a handful are established well enough to build a safe API around; the
rest are unverified: their signatures are unknown, and calling a guessed
signature against a private framework is undefined behavior waiting to
happen. `crates/gputools-replay-sys/src/inventory.rs` tracks this
distinction as data (`coverage()` currently returns established/total over
the exported surface), so the crate can state its own coverage rather than
imply completeness it does not have. Designing a safe, general API before
that surface is understood would bake in assumptions the campaign has not
yet earned.

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
