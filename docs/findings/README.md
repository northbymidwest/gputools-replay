# Findings: `GPUToolsReplay` reverse-engineering campaign

This tree is the campaign's evidence structure. Each of the 7 dossiers below
covers one surface of the framework (a related cluster of exports from
`gputools-replay-sys::inventory::EXPORTS`), plus a static class-dump reference.
`00-texture-fetch.md` starts populated with real findings carried over from
`docs/HANDOFF.md`; the rest start as templates for probes still to be run.

## Status table

Counts are per-symbol, drawn from each dossier's `## Status` section.

| # | Surface | Established | Unverified | Blocked | DeadEnd | Dossier |
| --- | --- | --- | --- | --- | --- | --- |
| 00 | Texture fetch | 6 | 0 | 0 | 0 | [00-texture-fetch.md](00-texture-fetch.md) |
| 01 | Playback | 3 | 0 | 0 | 0 | [01-playback.md](01-playback.md) |
| 02 | Harvester | 4 | 0 | 0 | 0 | [02-harvester.md](02-harvester.md) |
| 03 | Pipeline fetch | 1 | 0 | 0 | 0 | [03-pipeline-fetch.md](03-pipeline-fetch.md) |
| 04 | Shader profiler | 4 | 0 | 0 | 0 | [04-shader-profiler.md](04-shader-profiler.md) |
| 05 | Acceleration structure | 2 | 0 | 0 | 0 | [05-accel-structure.md](05-accel-structure.md) |
| 06 | Infrastructure | 10 | 1 | 0 | 0 | [06-infrastructure.md](06-infrastructure.md) |

Total: 30 established of 31 exported symbols. The one remaining, `GTMTLReplayClient_preferDevice`, is a resolved dead end in-process (dossier 06). Compare against
`gputools_replay_sys::inventory::coverage()`. This table is updated by hand as
dossiers move symbols between columns; when it disagrees with the crate's own
`coverage()` test, the crate is right and this table is stale.

Static reference for all surfaces: [`raw/classdump-27.txt`](raw/classdump-27.txt),
a live-runtime class dump taken on macOS 27 listing all 335 `GT*` classes
registered by the framework (Task 8). Each dossier's Static findings cites the
specific class entries relevant to its surface.

## The probe protocol

Every entry under a dossier's `## Live probes` section follows the same shape:
an **EXPECTATION**, written down *before* the probe runs, then a **RESULT**,
written after. A result that confirms the expectation is still recorded in
full, not just checked off, because a probe that surprises you later needs the
original expectation to show what was surprising.

Rules:

- **Expectation before result.** Never write the result first and back-fill an
  expectation to match it.
- **A claim marked MEASURED states how.** Name the probe binary, the capture,
  the parameters, and the date. "MEASURED" without a method is not a finding,
  it is an assertion wearing a finding's clothes.
- **Never generalize past what was observed.** One capture answering plane 0
  a certain way is a fact about that capture, not a fact about the field. See
  `docs/HANDOFF.md` section 3 for what this discipline caught (and missed)
  on the prior project.
- **Record nulls and surprises, not just hits.** A sweep that returns nothing,
  or returns something unexpected, is data. Silently discarding a null result
  is how the `plane:` question and the coverage-gap question in
  [00-texture-fetch.md](00-texture-fetch.md) exist in the first place; both
  came from someone writing down what actually happened instead of what was
  expected to happen.

## Session hygiene

The replayer is a shared, stateful, crash-prone resource. Before and after
*every* run against it:

```
pgrep -f GPUToolsReplayService
```

- **One session per process.** A second bootstrap in the same process aborts
  (exit 132). Never share a process across probes.
- **Serialize.** One replay session at a time, machine-wide. If another probe
  or the smoke binary might be running, wait.
- **Two-hour lockout.** An interrupted run orphans a session and locks the
  replayer for two hours. If `pgrep -f GPUToolsReplayService` still shows a
  process after a probe should have exited, recover with:

  ```
  gpudebug --terminate all
  pkill -9 -f GPUToolsReplayService
  ```

  Then re-check `pgrep -f GPUToolsReplayService` returns nothing before
  starting the next probe.
- Latency ranges from about 27 seconds to over 20 minutes. A slow run is not
  necessarily a hung one; do not kill a session on impatience alone.

## Dossiers

- [00-texture-fetch.md](00-texture-fetch.md) - populated: reply format, info
  offsets, live smoke result.
- [01-playback.md](01-playback.md) - template.
- [02-harvester.md](02-harvester.md) - template.
- [03-pipeline-fetch.md](03-pipeline-fetch.md) - template.
- [04-shader-profiler.md](04-shader-profiler.md) - template.
- [05-accel-structure.md](05-accel-structure.md) - template.
- [06-infrastructure.md](06-infrastructure.md) - template.
