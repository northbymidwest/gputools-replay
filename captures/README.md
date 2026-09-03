# Captures

Not committed (gitignored): `.gputrace` bundles are large (tens of MB each) and
are regenerated locally rather than stored. This directory holds them for local
work; only this README is tracked.

## Fixture captures - what the live tests use

The `gputools-replay` live tests (`crates/gputools-replay/tests/live_*.rs`, and
the `gputools-replay-hl` `live_hl_*.rs` tests) run against small, synthetic,
ground-truth captures generated from the Metal programs in `fixture-apps/`, so
the suite is self-contained and depends on no opaque third-party trace. Build
lines are in `fixture-apps/README.md`.

| capture | fixture | live test | classes |
| --- | --- | --- | --- |
| `known-textures-late.gputrace` | `known-textures.m` (late) | `live_textures` | textures + playback |
| `known-buffers.gputrace` | `known-buffers.m` (late) | `live_buffers` | buffers, heaps, pipelines |
| `known-draws.gputrace` | `known-draws.m` (late) | `live_draws` | wireframes |
| `accel-structure.gputrace` | `accel-structure.m` (late) | `live_accel` | acceleration structures |

Further `known-*` captures (depth, stencil, mips, 3D, ASTC, YCbCr, ambiguous)
back the format- and provenance-specific tests; see `fixture-apps/README.md`
for the full set.

Each live test is its own integration-test binary, so `cargo test ... --tests
-- --ignored` runs them in separate processes and none trips the
one-session-per-process guard. The synthetic captures answer no fetch by
default; see `docs/findings/00-texture-fetch.md` for why, and for the
`MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE=1` lever that fixes it.

## Operational note

Access to the replayer must be serialised, and an interrupted run can orphan a
session for up to two hours (`docs/HANDOFF.md` section 4). Read that before
running anything against a capture.
