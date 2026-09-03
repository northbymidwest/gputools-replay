# Surface: playback

> ## CORRECTION (2026-09-01, supersedes the stall analysis below)
>
> **Playback works in-process.** `play_to(1)` and `play_all()` both return
> cleanly in about a second on `small.gputrace`, and the controller's command
> index advances (0 -> 1 for `play_to(1)`, MEASURED by `probes/run.sh playstep`).
>
> Everything below headed "Hypothesis (INFERENCE) for why it never returns at
> 99% CPU", and the whole "Live experiment result" / GPU-execution-barrier
> conclusion, diagnosed a defect in OUR wiring as a property of the framework.
> The static reading of the commit path (the `waitForMTL4CPU` fence, the
> residency set, the config words) is not disproven - **it was never reached.**
>
> ### What was actually wrong
>
> The pointer being passed to `playAll`/`playTo` was not a controller.
>
> - `probes/src/session.rs` captured `GTMTLReplayController_init(pool)`'s
>   return value as the controller. That function does not vend one: it points
>   at the framework's `_GT_ENV` global and fills ~50 config bits from
>   `MTLREPLAYER_*` environment variables (already MEASURED during the
>   campaign, `_GTMTLReplayController_init` @ `0x24f8a0964`), and Apple's own
>   GPUToolsReplayService **discards** its return. The argument count was read
>   off the prologue and is right; the RETURN TYPE was assumed and never
>   established, and that assumption is the whole defect.
> - MEASURED (`vmmap` on a stalled probe): that pointer, `0x280488ff8`, lies
>   inside GPUToolsReplay's read-only `__AUTH_CONST` segment
>   (`0x2804857f0-0x2804a6aa8`), not on the heap. The real controller was
>   `0x1010ec028`, an ordinary heap address.
> - MEASURED (`sample` on a stalled probe, the evidence static RE could not
>   reach): the spinning main thread was NOT in a fence wait. Its stack was
>   `play_to` -> `GTMTLReplayController_playTo+240` -> `_sigtramp` ->
>   `HandleCrashSignal` -> `_sigtramp` -> `HandleCrashSignal` -> ... The
>   framework's own crash-signal handler caught the fault and then faulted
>   itself, and that signal-handler recursion is what pegged a core forever.
>   From the outside it was indistinguishable from a spinning CPU fence poll.
> - The faulting PC resolves to static `0x24f84db84` (runtime `0x253149b84`,
>   slide `0x38FC000`): the exit test's first load of `[controller+0x5820]`,
>   i.e. the very first controller dereference `playTo` performs. See the
>   "Termination" section below, which names that same address.
>
> ### The controller
>
> Field 1 of the `GTMTLReplayClient` struct, byte offset 8, filled by
> `GTMTLReplayClient_init`. The type encoding of `-[GTMTLReplayService
> initWithContext:]` - the same string the publication-gate regression test
> pins - types it:
>
> ```text
> {GTMTLReplayClient=^{apr_pool_t}^{GTMTLReplayController}Q{?=QQQdII}...}
> ```
>
> field 0 the APR pool, field 1 the controller. Read it with
> `ClientBuffer::controller`; `GTMTLReplayController_init`'s return is now
> declared `*mut c_void` so it cannot be mistaken for one again.
>
> ### What this does and does not establish
>
> ESTABLISHED: `playTo`/`playAll` run to completion in-process, unentitled and
> headless, and advance the command index. The two-hour-lockout and
> stuck-probe operational warnings for playback no longer apply.
>
> NOT established: that the replayed commands executed *correctly*. Nothing
> here validates replayed output. `MTLREPLAYER_FORCE_RESOURCES_RESIDENT` and
> the residency analysis were never actually exercised, since the crash
> preceded all of it, so their real effect is once again unknown rather than
> ruled out.
>
> COVERAGE GAP: **ANSWERED (2026-09-01), and the answer is no.** Playback does
> not change which resources a fetch can reach. The apparent post-playback ref
> shift was a third bug, in our probe rather than the framework: it diffed
> record field 0x00, which is a per-session request ordinal, not the streamRef
> (field 0x08). See the CORRECTION in
> [00-texture-fetch.md](00-texture-fetch.md).
>
> With the probe reading the streamRef, MEASURED on both committed captures:
>
> | capture | refs answering before | after `play_all()` | diff |
> | --- | --- | --- | --- |
> | `small.gputrace` (226 commands) | 4 of 2001 | 4 of 2001 | 0 new, 0 dropped |
> | `corpus.gputrace` | 180 of 2001 | 180 of 2001 | 0 new, 0 dropped |
>
> `playto:1` gives the same null result. So the long-standing hypothesis - that
> a resource only becomes fetchable once playback advances past the command
> that creates or writes it - is DISPROVEN on every capture available here.
> The fetch coverage gap is real and is NOT explained by playback position.
> Anyone resuming should look elsewhere for it, and should not re-run this
> experiment expecting a different answer.

Symbols:

- `GTMTLReplayController_playAll` (C symbol) - 1 arg (controller), Established
  from prologue; semantics inferred (play to end). Runs to completion in-process
  (MEASURED, see CORRECTION). See Static findings.
- `GTMTLReplayController_playTo` (C symbol) - 2 args (controller, uint32 target
  command index), Established from prologue; forward-only replay-loop semantics
  established from control flow AND now by probe: `play_to(1)` returns cleanly
  and advances the command index at +0x5820 from 0 to 1 (MEASURED,
  `probes/run.sh playstep`). It did NOT resolve the texture-fetch coverage gaps
  in [00-texture-fetch.md](00-texture-fetch.md): playback leaves fetch coverage
  unchanged on both committed captures (see the CORRECTION above).
- `GTMTLReplayController_rewind` (C symbol) - 1 arg (controller), Established
  from prologue; semantics inferred (teardown + restore initial state) and now
  CONFIRMED live: after `play_to(5)` leaves the command index at 5, `rewind()`
  returns cleanly and the index reads 0 again (`probes/run.sh playstep
  captures/small.gputrace 5 rewind`). What it tears down beyond the index is
  still unestablished.

In all three cases the controller argument must come from
`ClientBuffer::controller` (client struct field 1), NOT from
`GTMTLReplayController_init`'s return value. See the CORRECTION above.

## Static findings

Disassembly of `GPUToolsReplay` (arm64e framework Mach-O extracted from the
dyld shared cache) via `ipsw macho disass`. Arg COUNT is read off each
prologue (which of x0..x7 are saved before first use): high confidence. Arg
ROLE / SEMANTICS is inferred from register use and labelled as such.

- `GTMTLReplayController_init` (Established, see
  [00-texture-fetch.md](00-texture-fetch.md) and `docs/HANDOFF.md` 2.2) is a
  one-argument function taking the APR pool. The three symbols in this dossier
  are indeed methods on the same controller object: all take the controller in
  x0 and dereference `[x0]` as its state/vtable pointer, and all three touch the
  same controller field, a current-command-index at byte offset **0x5820**.

- **`_GTMTLReplayController_playAll` @ 0x24f84df4c: 1 arg (Established).** The
  whole body is 5 instructions and never reads x1/x2 as inputs. It loads a
  command count out of controller state (`ldr x8,[x0]; ldr x8,[x8,#0x98];
  ldr w1,[x8,#0xc]`), sets `w2=0`, and tail-calls
  `_GTMTLReplayController_debugSubCommandStop(controller, count, 0)`.
  - Role: x0 = controller (Inferred). What it does (Inferred): "play to the
    end" - it drives the shared stop-at-subcommand primitive with the full
    command count.
  - Helper `_GTMTLReplayController_debugSubCommandStop` @ 0x24f84df60 takes
    **3 args** (prologue saves x2->x20, x1->x21, x0->x19): controller, a
    command index/count (x1, used as `w1-1` into
    `_GroupBuilder_findExclusiveRange`), and a small flag/mode (x2). It logs to
    `_g_activityLog`, calls `_GTMTLReplayController_debugSubCommand`, and on a
    begin-command-buffer boundary calls
    `_restoreIndirectResourceUsageForCommandBuffer`. It reads/writes the
    controller index at +0x5820.

- **`_GTMTLReplayController_playTo` @ 0x24f84da94: 2 args (Established).**
  Prologue saves `x1->x19` (0x24f84dab4) and `x0->x20` (0x24f84dab8); no x2 is
  read as an input (internal calls set x2 themselves to 0/-1).
  - x0 = controller (Inferred; `add x23,x0,#0x5000` forms a state pointer,
    `[x20]` is dereferenced as vtable/state at 0x24f84db90).
  - x1 = target command index, uint32 (Strong inference). It is emitted as the
    `targetIndex` field of the log line
    "playTo - currentIndex: %6d targetIndex: %6d" (`stur w19,[sp,#0x1a]`), and
    it is the loop bound: current index at controller+0x5820 is compared to w19
    (`cmp w8,w19; b.hs`/`b.lo`).
  - What it does (Established control flow): a forward-only replay loop from the
    controller's current index up to (not including) x1. Each iteration fetches
    a 64-byte command record (base `[[controller]+0x98]+0x18` +
    `currentIndex*0x40`), calls `_updateCommandEncoder` (plus
    `_restoreCommandBuffer` on a begin-command-buffer, and one of
    `_executeCommandsInBuffer` / `_defaultDispatchFunction`), then increments
    the index at +0x5820. If the current index already >= x1 the body is
    skipped: playTo never rewinds. This matches the
    [00-texture-fetch.md](00-texture-fetch.md) coverage-gap hypothesis
    mechanically (advancing the index is how a not-yet-created resource would
    become fetchable), though that link is still unproven by probe.

- **`_GTMTLReplayController_rewind` @ 0x24f89efa8: 1 arg (Established).**
  Prologue saves `x0->x19`; never reads x1. Logs "Rewinding" then calls
  `_Rewind(controller)` and returns (a thin logging wrapper).
  - `_Rewind` @ 0x24f89fb40 (1 arg): if the current index (+0x5820) is 0 it is a
    no-op; otherwise it calls `_RewindWithoutRestore(controller)` then rebuilds
    initial state (`_AppendRestoreJobsToLoadQueue`, `_SignalLoadQueueThreads`,
    `_RestoreResourcesFromBuffer`, `_RestoreOrderedResourcesFromArchive`,
    `_RestoreVisibleFunctionTablesForFunctionIndex`,
    `_RestoreIntersectionFunctionTablesForFunctionIndex`).
  - `_RewindWithoutRestore` @ 0x24f89f158 (1 arg) is the teardown half: frees
    tile memory (`_GTMTLReplayController_tileMemoryFree`) and tears down the
    controller resource arrays without restoring captured contents.

- These are C symbols, not ObjC classes, so they do not appear in
  `docs/findings/raw/classdump-27.txt` (that dump only lists ObjC classes).

## Open questions

- Arg COUNT is now established for all three (playAll 1, playTo 2, rewind 1).
  playTo's x1 is very likely a raw uint32 command index (loop bound + log
  field), but whether public callers pass a raw index or a UID resolved
  elsewhere is not provable from these functions alone. Open.
- The meaning of debugSubCommandStop's x2 flag (0 from playAll) and of
  controller field +0x5824 (cleared at playTo exit) is undetermined.
- Whether `playTo` genuinely explains the [00-texture-fetch.md](00-texture-fetch.md)
  coverage gap (`small.gputrace`: 3 of 7 textures never answer; a 182-record
  sweep drops to 180 at larger regions) is now mechanically plausible
  (advancing the +0x5820 index is what makes later-created resources exist), but
  still a hypothesis: no probe has called `playTo` and re-run a fetch sweep.
- Whether playback must run before any fetch, or only affects which resources
  become fetchable, is unknown.

## Replay correctness (2026-09-01, MEASURED)

This section began as a strong negative claim and was CORRECTED the same day by
better evidence. Read it as: the fetch+decode path is validated against ground
truth; the GPU genuinely executes replayed work; and in-process playback
preserves used-texture content on real captures. A synthetic fixture shows one
resource degrade, but that does not generalise.

**1. Fetch + reply-decode is byte-correct against ground truth (ESTABLISHED).**
`fixture-apps/known-textures.m` fills each texture with one exact solid colour.
The textures the capture keeps an end-of-capture snapshot of (the blit source
and destination) fetch and decode to EXACTLY that colour - every pixel, zero
mismatches - via `probes/run.sh pixelcheck`. First validation of the fetch path
against known pixels rather than a record count. Belongs to
[00-texture-fetch.md](00-texture-fetch.md); recorded here because the same probe
raised the playback question. This claim is unaffected by everything below.

**2. The GPU executes the replayed work, and it passes Metal validation
(ESTABLISHED).** A `play_all()` run under `MTL_DEBUG_LAYER=1` + shader/GPU
validation and the replayer's own `MTLREPLAYER_REDIRECT_LOGGING_TO_STREAMS=1`
(scratch `diag.log`): "Metal API Validation Enabled", "Metal GPU Validation
Enabled", then 19 command buffers commit - named
`com.apple.gputools.replay.{RestoreResourcesFromArchive, TextureBlit,
FetchResourceObjectBatch}` - with the GPU fence value climbing monotonically
(v:1 -> 18) and every MTL4-before-MTL3 wait satisfied. **Zero** validation
errors, faults, hazards, or aborts. So the old "the committed buffers never
execute on the GPU" hypothesis is disproven for a third time: replay submits
valid Metal work that the GPU runs to completion.

**3. On REAL captures, playback preserves used-texture content (MEASURED,
`probes/run.sh datadiff`).** Per-record payload comparison before vs after
`play_all()`:

| capture | unchanged | changed | nature of the change |
| --- | --- | --- | --- |
| `small.gputrace` | 3 of 4 | 1 (ref 24) | 0.2% of bytes; stays a real image |
| `corpus.gputrace` | 177 of 180 | 3 | stay real images; 2 are exactly the duplicate-`resourceIndex` pair (1121/1122) |

No used texture collapses to a uniform fill. Playback is not inert (the record
set is stable but some payloads DO change, so fetch reflects post-replay state,
not a frozen snapshot), yet used content survives essentially intact. There is
no evidence of replay INCORRECTNESS on real data.

**4. The synthetic fixture shows one resource degrade - a fixture artifact, not
a general result (MEASURED, and the correction to this section's first draft).**
On `known-textures-late.gputrace`, `private_blit_dst` is byte-perfect from its
snapshot BEFORE playback and reads as a uniform `ff00ffff` placeholder AFTER.
But the fixture is a degenerate capture: almost every texture is an "unused
resource" (no captured command touches it), and the replayer log shows the
after-playback fetch of the unused refs (6-9) failing `Metal object creation
(ResourceUnused, Code 150)` and losing their labels, i.e. the playback rebuild
drops them. `blit_dst` is the sole genuinely-used texture, and in that
unused-dominated context its content is not restored after the rebuild. The
placeholders even split by storage mode - Private -> `ff00ffff`, Shared ->
zeros - i.e. deliberate uninitialised-resource fills, not GPU garbage. The
real-capture result (3 above) is the control that shows this does not
generalise; do not read the fixture as "replay produces wrong pixels".

Honest limits:

- **Oracle pixel comparison: DONE for a real texture** (2026-09-01,
  `probes/run.sh texbmp` + `gpudebug`). Our fetch of `small.gputrace` ref 24
  matches `gpudebug`'s own fetch of the same texture pixel for pixel, once
  `gpudebug`'s Generic-RGB display gamma (best-fit `ours ** 0.84`, ~1% residual)
  is removed - see [00-texture-fetch.md](00-texture-fetch.md). Before- and
  after-playback fetches match the oracle equally, so "used content survives
  playback" is now corroborated against a second implementation, not only by
  before/after self-comparison. (`gpudebug` still cannot fetch the synthetic
  fixture, so the fixture's used texture has no oracle.)
- **The `blit_dst` mechanism is not fully pinned** - why the one used texture in
  an unused-dominated capture is not restored after the rebuild. It is a narrow,
  synthetic-only question; the real-capture behaviour is the one that matters
  for the crate.

Implication for the safe crate: fetch of stored contents is trustworthy and
shippable; playback executes real GPU work and preserves used content on real
captures. "Playback works" can now mean more than "the call returns", though a
rigorous oracle-pixel check on a real capture is still the missing proof.

## Live probes

### playAll before a fetch (coverage-gap hypothesis) - INCONCLUSIVE, playAll does not return

- Probe: `probes/src/bin/playback.rs`, mode `playall`, capture
  `small.gputrace`, natural-size sweep streamRef 0..=2000.
- EXPECTATION: a baseline sweep answers 4 of 7 textures (the display drawable
  and resourceIndex 108/109/111 never answer, per `captures/README.md`); after
  `GTMTLReplayController_playAll(controller)`, a second identical sweep might
  let some of those resources answer, which would explain the coverage gap.
- RESULT (MEASURED 2026-09-01, macOS 27.0):
  - Baseline sweep answered **4 of 2001** streamRefs, matching the smoke run
    exactly (all BGRA8Unorm). So bootstrap, load, and the controller wiring are
    correct: the probe reached `play_all()` without crashing.
  - `GTMTLReplayController_playAll(controller)`, called as a **blocking,
    synchronous, in-process FFI call, did NOT return.** The process busy-looped
    at ~99% CPU (state R, one core pegged) for over 70 minutes, far past the
    ~20-minute latency ceiling documented in HANDOFF section 4, then was killed
    (SIGKILL). It was not a 0%-CPU deadlock; it was a live compute loop that
    never reached its termination condition.
  - Recovery was CLEAN: after the kill, `gpudebug --terminate all` reported
    "No active sessions" and no `GPUToolsReplayService` was running. Because
    this crate drives the replayer **in-process** (not through the XPC service),
    killing our own process released the session with NO two-hour lockout. This
    is a distinct operational finding from HANDOFF section 4, whose lockout
    warning applies to the XPC-service path.
- CONCLUSION: `playAll` is not usable as a naive blocking in-process call in
  this unentitled setup; it does not terminate, so the coverage-gap hypothesis
  remains UNTESTED (playback never completed, the second sweep never ran).
- OPEN QUESTION raised by this result: what does `playAll`'s replay loop wait
  on that never arrives here? Candidates, none established: it needs the run
  loop pumped (the same lesson as fetch, which must pump rather than
  `waitUntilCompleted`); or it dispatches per-command GPU work whose completion
  never signals without a proper/entitled device; or it must be driven on a
  different thread while the caller pumps. Resolving this needs deeper static
  analysis of the replay loop's exit condition (decompile
  `_GTMTLReplayController_debugSubCommandStop` and the loop around
  controller+0x5820) before any further live playback attempt. Do NOT re-run a
  blocking `playAll`/`playTo` live until the termination mechanism is understood.

## playAll termination analysis

STATIC only (no live runs). Binary: extracted arm64e `GPUToolsReplay`
Mach-O, `__TEXT.__text` = 0x24f7b37b0..0x24fbc7190. Every address below is
cited from `ipsw macho disass`, cross-checked against Ghidra pseudo-C
(`scratchpad/re/decompiled.c`); the two agree.

### The call chain has no unbounded outer loop, and no recursion

- `_GTMTLReplayController_playAll` (0x24f84df4c) is 5 instructions: it loads a
  command count `N = [[controller]+0x98]+0xc` (0x24f84df50-54) and tail-calls
  `_GTMTLReplayController_debugSubCommandStop(controller, N, 0)` (0x24f84df5c).
- `_GTMTLReplayController_debugSubCommandStop` (0x24f84df60) is NOT a loop and
  is NOT recursive. It logs, calls
  `_GTMTLReplayController_debugSubCommand(controller, N, 0)` ONCE (0x24f84dfa4),
  does a `GroupBuilder_findExclusiveRange` /
  `restoreIndirectResourceUsageForCommandBuffer` fixup, and returns.
- `_GTMTLReplayController_debugSubCommand` (0x24f84f378) is a large but
  single-pass body (seek-to-position + memoryless-texture + attachment-format
  setup). It calls `_GTMTLReplayController_playTo` ONCE per invocation (at one
  of two mutually-exclusive sites, decompiled.c:548/551). Its only backward
  branches are small fixed-count inner loops (two 8-iteration texture loops,
  one memoryless-tile-size loop), none unbounded.
- REFUTES the "unbounded recursion" hypothesis for the playAll path. The
  self-recursion `debugSubCommandStop(ctrl, i-1, ...)` /
  `debugSubCommandStop(ctrl, i, flag-1)` lives in a DIFFERENT function,
  `_GTMTLReplayController_debugSubCommandResume` (0x24f85016c,
  decompiled.c:706/709), which is NOT reached from `playAll`. PROVEN by grep:
  neither `debugSubCommand` nor `debugSubCommandStop` contains a `bl` to
  `debugSubCommandStop` (0x24f84df60), `debugSubCommandResume` (0x24f85016c),
  or `debugSubCommand` (0x24f84f378) itself (all `bl` targets in their bodies
  are either the named single-pass callees above or out-of-slice `0x2500xxxx`
  thunks). So `playAll` executes a straight-line chain into one bounded loop;
  there is no recursion to run away.

### The one replay loop (PROVEN bounded and monotonic)

The single replay loop is in `_GTMTLReplayController_playTo` (0x24f84da94).
The loop index is a u32 at **controller+0x5820** (accessed as `[x23,#0x820]`
where `x23 = controller + 0x5000`, set at 0x24f84dabc; this is the same field
the decompiler renders as `param_1[0xb04]`, since `0xb04*8 = 0x5820`). The
decompiler shows the loop verbatim as `do { ...; uVar1 = param_1[0xb04]+1;
param_1[0xb04] = uVar1; } while (uVar1 < param_2)` guarded by `if
(param_1[0xb04] < param_2)` (decompiled.c:43-63).

- Precondition / exit test (0x24f84db84): `w8 = [+0x5820]; cmp w8, w19; b.hs
  0x24f84dc28`. `w19` is the target index (arg1). If `currentIndex >=
  target`, the loop is skipped/exited.
- Body (0x24f84db9c..0x24f84dc0c) per iteration: load the 0x40-byte command
  record at `[[controller]+0x98 +0x18] + currentIndex*0x40`; if
  `GTFenum_isBeginCommandBuffer`, `restoreCommandBuffer`;
  `updateCommandEncoder`; then EITHER
  `executeCommandsInBuffer(controller,cmd,0,-1)` (0x24f84dbf8, indirect-execute
  path) OR `defaultDispatchFunction(controller,cmd)` (0x24f84dc08, ordinary
  command).
- Increment / back-edge (0x24f84dc14-0x24f84dc24): reload `[+0x5820]`, `add
  #1`, store back, `cmp target; b.lo 0x24f84db9c`. The increment is
  UNCONDITIONAL (not gated by any dispatch result).

Therefore the loop runs exactly `target - currentIndex` iterations and
terminates. PROVEN: no inner replay call rewrites +0x5820 mid-loop -
`grep` for stores to `#0x820`/`0x5820` across
`defaultDispatchFunction`, `executeCommandsInBuffer`, `updateCommandEncoder`,
`restoreCommandBuffer`, `rewind` finds NONE; the only writers are playTo's own
increment (0x24f84dc1c) and debugSubCommand's post-playTo index set
(0x24f84fe40 / 0x24f84fe74). So the 99% CPU non-return is NOT a stuck loop
counter.

### The spin/wait point (per-command-buffer CPU fence wait)

The stall is inside the per-command dispatch, on command-buffer-**commit**
commands. `defaultDispatchFunction` (0x24f857d60) is single-pass (its two
backward branches at 0x24f8583a0/0x24f8583cc merely rejoin the shared
"Acceleration structure indirection is not supported" warning tail back into
straight-line handling; not a cycle) and forwards to
`_GTMTLReplayController_defaultDispatchFunction_noPinning` (0x24f857c10), a
giant switch over the recorded Metal call enum. On a commit command it calls:

- `_GTMTLReplay_commitCommandBuffer` (nop.s:215, 0x24f858718), or
- `_GTMTLReplay_commitCommandBufferAndWaitUntilSubmitted` (nop.s:5787,
  0x24f85dd40), or
- `_GTMTLReplay_commitMTL4CommandBuffers` (0x24f8589b8).

BOTH commit functions perform a CPU-side fence wait as their first act:
`_GTMTLReplay_commitCommandBuffer` (0x24f7fd8ac) and
`_GTMTLReplay_commitCommandBufferAndWaitUntilSubmitted` (0x24f7fda60) each do
`ldr x0,[cmdbuf,#0x28]; mov w1,#0x1f40; bl _GTMTLCoreSync_waitForMTL4CPU`
(0x24f7fd8d4-e4 and 0x24f7fda88-98), then install completion/restart handler
blocks (`AddHandlers`, `___GTMTLReplay_addGPURestartHandler_block_invoke`) and
commit (0x250001920) + a `waitUntil...` (0x250002500 / 0x250002530).
`_GTMTLCoreSync_waitForMTL4CPU` (0x24faa2adc) reads the wait-target value
`[obj+0x20]` and tail-calls `_GTMTLCoreSync_waitForValueCPU(obj, value, 0x1f40,
label)` (0x24faa2ae8). `waitForValueCPU` (0x24faa29e8) delegates the actual
wait to a helper at 0x25000c2c0 with `(primitive=[obj], value, timeout=0x1f40)`
(0x24faa2a60-6c). So EVERY committed command buffer waits on a fence/shared-event
value reaching a target, with a 0x1f40 (=8000) timeout argument.

### Hypothesis (INFERENCE) for why it never returns at 99% CPU

The replay commits each recorded command buffer and then waits (CPU-side) for a
fence value that is advanced by GPU **completion** of that command buffer. Run
synchronously in-process in this headless/unentitled setup, the committed work
is never actually executed-and-signalled by a device (the completion/restart
handler blocks installed just before the wait never fire), so the fence value
never reaches its target. `waitForValueCPU` therefore does not satisfy quickly;
the observed ~99% CPU (a busy spin, not a 0% blocked wait) indicates the wait
primitive polls the value rather than parking. Either it spins unbounded on the
first unsatisfiable commit, or it spins to the 0x1f40 timeout per commit and a
capture with many command buffers accumulates into the 70+ minute apparent
hang; both converge on the same root cause. This is consistent with the Live
probe (99% CPU, no return) and DISTINCT from the async-fetch stall (0% CPU),
because this is a CPU fence-poll, not a run-loop-gated wait - which is why
"just pump the run loop" is not expected to fix this path.

### What a correct caller likely must do (candidates, grounded)

- Drive the replay against a real Metal device/queue whose submitted command
  buffers actually execute and signal the shared event at `cmdbuf+0x28`
  (target value `[obj+0x20]`), so `waitForValueCPU` is satisfied. The commit
  path (0x250001920) and its completion/GPU-restart handler blocks must
  actually run; today they do not, so the fence never advances.
- Because the wait is CPU-fence-based rather than run-loop-based, pumping an
  NSRunLoop on the calling thread alone is NOT expected to release it (unlike
  the fetch path). If a secondary mechanism drives GPU completion/handlers, the
  replay would likely need to run on a thread that lets those handlers execute.

### Open questions (unproven)

- The wait primitive 0x25000c2c0 (and `waitForValueCPU`'s inner behaviour) lies
  OUTSIDE this extracted slice (target > `__text` end 0x24fbc7190; it is in
  another dyld-shared-cache dylib), so spin-vs-block and the unit of the 0x1f40
  timeout (8000 us? ms? poll count?) are NOT proven here.
- Whether the fence value ever partially advances (slow forward progress) or is
  wholly stuck on the first commit.
- What `cmdbuf+0x28` / `[obj+0x20]` concretely is (MTLSharedEvent?) and which
  component is responsible for signalling it during a correct replay.

## Can playback execute in-process?

STATIC VERDICT was **(a) LIKELY YES**, but a LIVE EXPERIMENT (2026-09-01, see
"Live experiment result" below) CONTRADICTS it: even `play_to(1)`, a single
command, does not complete in-process. Read the static reasoning below as
"necessary but demonstrably not sufficient": the device/queue/fence being
standard Metal does not, on its own, make the committed buffers complete here.

Static reasoning (still valid as far as it goes) - the device, queue, command
buffer, commit, and fence are all standard in-process Metal, so a real device
that executes the committed work would satisfy the wait. The load-bearing
facts (beyond the termination analysis above):

- **The device and queues the replay commits to are REAL, not null/capture
  stubs (Established).** `_DEVICEOBJECT` (0x24faa21f4) unwraps a wrapper chain
  down to the underlying `MTLDevice`; `GTMTLReplayObjectMap` holds
  `_defaultDevice` and a `_devices` array and vends real
  `-defaultCommandQueue` (0x24f8beee8), `-defaultCommandQueue4` (0x24f8bef10,
  the MTL4 queue), and `-mtl4CommandQueueForKey:` (0x24f8b9450).
  `_EnableDeviceConfiguration` (0x24f7fe4c4) queries the device for GPU-family /
  feature-support strings and configures it - a real hardware device.
- **The fence is a normal command-buffer / MTLSharedEvent completion, not a
  bespoke event only an entitled XPC service can post (Established structure).**
  `_GTMTLReplay_commitCommandBuffer` installs standard completion-handler blocks
  via `_AddHandlers` (0x24f7fd8fc) and a GPU-restart completion block typed
  `void (^)(id<MTLCommandBuffer>)` (descriptor
  `___block_descriptor_40_e28_v16?0"<MTLCommandBuffer>"8l`), then commits the
  real buffer. The value the CPU polls (`cmdbuf+0x28` reaching `[event+0x20]`)
  is driven off GPU completion of that same real buffer. Nothing in this path
  requires the XPC/entitled service to post the signal.
- **Basic Metal command-buffer submission needs no special entitlement**
  (the GPUTools entitlements gate capture and the tooling XPC channel, not
  `commit`/`waitUntilCompleted`). So an unentitled in-process process CAN
  execute the committed work, and a real executing device WILL fire the
  completion handler and advance the shared event that `waitForValueCPU` polls.
- **The stall is consistent with per-commit timeout accumulation, not an
  unsatisfiable-by-design fence.** The per-commit wait timeout is a bounded
  constant `0x1f40` (=8000), and the out-of-slice wait helper `0x25000c2c0`
  takes that timeout and returns a bool (the `waitUntilSignaledValue:timeoutMS:`
  shape). If each committed buffer fails to complete-and-signal, each wait burns
  its full timeout and N command buffers accumulate into the observed 70+ min.

### Proposed experiment (single, most-promising; NOT run here)

Under the existing unlock (`MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX`), call
`play_to(1)` (ONE command) on `small.gputrace` with `MTLCAPTURE_LOG_ERRORS=1`
and `MTLREPLAYER_STOP_ON_COMMIT_ERROR=1`, and time it:
- Returns sub-second with no commit error -> the single buffer executed and
  signalled; in-process execution WORKS and the full-frame stall is pure
  accumulation. Fix = per-commit speed (already executing); then scale up.
- Hangs ~8s then returns with a commit error -> execution is attempted but the
  buffer fails to complete (resource patching / validation); the fix is on the
  commit/resource side. Optionally pin a known-good device first with
  `MTLREPLAYER_OVERRIDE_DEVICE_REGISTRY_ID=<registryID of
  MTLCreateSystemDefaultDevice()>`.

This isolates the crux (does ONE buffer execute+signal) from the accumulation
(N x 8s). Separately, `GTMTLReplayClient_preferDevice(client)` takes a single
argument (the client; see [06-infrastructure.md](06-infrastructure.md)) and can
be called after `GTMTLReplayClient_init` and before playback to bind the chosen
real device deterministically alongside the registryID override.

### Live experiment result (2026-09-01, macOS 27.0): play_to(1) does NOT complete

The experiment above was run: `probes/run.sh playback small.gputrace 2000
playto:1` with `MTLCAPTURE_LOG_ERRORS=1` and
`MTLREPLAYER_STOP_ON_COMMIT_ERROR=1`.

- Baseline sweep answered **4 of 2001** (fetch path works, as always).
- `play_to(1)` (a SINGLE command) then **busy-looped at 100% CPU for 9+ minutes**
  (state R), and was killed. Recovery was clean again: `gpudebug --terminate all`
  reported "No active sessions", no lockout.
- **Neither predicted branch occurred.** It did NOT return sub-second (so the
  buffer did not execute-and-signal), and it did NOT bound at ~8s and stop
  (`MTLREPLAYER_STOP_ON_COMMIT_ERROR=1` did not stop it, and one 8s timeout
  cannot explain 9+ minutes). So the "per-commit 8s-timeout accumulation across
  N buffers" model is REFUTED for the single-command case: the CPU-side wait does
  NOT give up after 8s and advance; it retries indefinitely at 100% CPU.
- CONCLUSION (MEASURED): in our headless, unentitled, in-process setup a
  committed replay command buffer does NOT complete/signal, even for ONE command,
  and `MTLREPLAYER_STOP_ON_COMMIT_ERROR` surfaced no commit error before the
  hang. The static "LIKELY YES" is not borne out: something prevents the GPU work
  from completing that the standard-Metal static picture does not capture.
- REFINED OPEN QUESTION (the real barrier, unresolved): why does the buffer never
  complete? Candidates, none established: (1) the unentitled in-process device
  accepts the commit but the GPU never actually executes the replayed buffer (an
  execution the entitlement/XPC service may be what enables); (2) the buffer
  references resources/state not restored, and it stalls (not errors) rather than
  failing cleanly; (3) a required setup step is missing before playback (e.g.
  `preferDevice`, an explicit `MTLREPLAYER_OVERRIDE_DEVICE_REGISTRY_ID`, a
  drawable/surface, or driving completion on a pumped queue). Distinguishing
  these needs deeper static work on the commit-and-complete path (the out-of-slice
  wait helper `0x25000c2c0` and the completion handler) and possibly a comparison
  with how the entitled XPC service drives the same commit. Do NOT re-run a
  blocking playback probe until one of these is understood; it will just stall.

### Service vs in-process diff: the missing configuration step (STATIC)

Binaries: the XPC
services under
`GPUToolsDeviceServices.framework/.../XPCServices/{GPUToolsReplayService,
GPUToolsDebugService}.xpc`, and the extracted `GPUToolsReplay` framework (FW).

VERDICT: **(A) a missing setup step, not (B) an entitlement gate.** The in-process
fetch already commits and completes a real GPU command buffer unentitled (see
next paragraph), so GPU execution is reachable in-process; what our bootstrap
omits is the replay-session CONFIGURATION the client normally performs before
driving replay.

- **`GPUToolsReplayService.xpc` does NOT drive replay.** Its `start` (MEASURED,
  `ipsw macho disass --entry`) does `apr_initialize` ->
  `apr_pool_create_ex` -> `GTMTLReplayController_init(pool)` ->
  `GTMTLReplayClient_createNewTransport(&struct)` ->
  `xpc_main(handler@0x100000a5c)`, and that handler is one instruction,
  `b _xpc_connection_cancel` (it rejects every connection). It is a
  transport/memory broker (strings `com.apple.gputools.transport`,
  `failed to mark memory(GRAPHICS)`; imports `mach_make_memory_entry_64`,
  `mach_memory_entry_ownership`, `vm_map`), not the replay driver. The heavy
  engine is statically linked into the sibling `GPUToolsDebugService.xpc`
  (20 MB), launched by the dev-mode-gated daemon `gputoolsserviced`
  (`com.apple.gputools.service`). Apple drives replay OVER XPC into that entitled
  server; there is no in-process reference implementation to copy.
  `createNewTransport` (FW `0x24f8d5114`) is an NSXPC client connection to
  `com.apple.gputools.replay` with `GTMTLReplayService` + `GTBulkDataService`
  remote proxies (see [06-infrastructure.md](06-infrastructure.md)).

- **Fetch proves GPU execution + completion works in-process unentitled.**
  `-[GTMTLReplayService fetch:]` -> `_FetchResourceObjectBatch` ->
  `___FetchResourceObjectBatch_block_invoke` installs an
  `addCompletedHandler:`-shaped block (descriptor `v16?0"<MTLCommandBuffer>"8l`)
  on a real command buffer, and our fetch path completes in-process. The replay
  device is a plain `MTLCreateSystemDefaultDevice`/`MTLCopyAllDevices` (FW
  imports); device creation is not entitlement-gated. This REFUTES (B): the
  barrier is not "unentitled processes cannot execute replayed GPU work."

- **The concrete gap: MTL4 residency-set attachment is gated on a config flag we
  never set.** The playback commit is MTL4 (`commitMTL4CommandBuffers`,
  `waitForMTL4CPU`, `signalGPU_MTL4`). MTL4 replay needs an explicit residency
  set (`MTLResidencySetDescriptor`, `AddTraceBuffersToResidencySet`,
  `-[GTMTLReplayObjectMap addGlobalResidencySetToQueue:]`).
  `addGlobalResidencySetToQueue:` (`0x24f8b6300`) begins
  `ldrb w8,[0x276aaf289]; tbnz w8,#3,...; else ret` -- if flag byte
  `0x276aaf289` bit 3 is clear it returns WITHOUT binding the residency set to
  the queue, and BSS-zero default is clear. That flag word (`0x276aaf288`
  cluster) is written by `GTMTLReplayClient_preferDevice` (`0x24f7fe12c/158`) and
  by `-[GTMTLReplayService update:]` (block `.321` at `0x24f80a56c`, twelve
  `orr;str [x20,#0x288]` with `x20 = 0x276aaf000`, bits sourced from the client's
  update message: `ldrb [x1,#0x18]`, `[x0,#0x3c]`). `load:error:` does NOT write
  it. Our `initWithContext:` -> `load:` -> bare C `playAll` path sends neither
  `preferDevice` nor `update:`. (CORRECTION, see "The residency-config lever"
  subsection below: `_GTMTLReplayController_init` DOES write this bit on every
  bootstrap, from env var `MTLREPLAYER_FORCE_RESOURCES_RESIDENT`, defaulting to
  0 - so it is the env default, not the absence of a writer, that leaves bit 3
  clear.) With that env unset the flags stay zero, the global MTL4 residency
  set is never attached to the queue, the committed captured buffers cannot be
  completed by the GPU, and the GPU-only CoreSync fence (`cmdbuf+0x28`, no CPU
  signal path exists: FW has `signalGPU_MTL3/MTL4` but no `signalCPU`) never
  advances -- so `waitForMTL4CPU` spins at 100 percent CPU forever. This matches
  the `play_to(1)` symptom exactly (100 percent CPU, no commit error, no 8s
  bound) and explains why fetch (its own resident readback buffer) works while
  playback (the trace's global residency set) does not.

- **Proposed experiment (single, most promising; not run here).** After
  `GTMTLReplayClient_init`, call `GTMTLReplayClient_preferDevice(client)`
  (one arg, the client; see [06-infrastructure.md](06-infrastructure.md)),
  optionally with `MTLREPLAYER_OVERRIDE_DEVICE_REGISTRY_ID` set to a real
  device's registryID; if still hanging, send a minimal
  `-[GTMTLReplayService update:]` before `play_to(1)`, and re-time. Sub-second
  return means the config/residency wiring was the gap. Do NOT re-run a blocking
  `playAll` until at least `preferDevice` is wired in.

- **Open (not provable from these binaries):** whether any additional in-process
  gap remains once residency is attached (e.g. captured cross-queue CoreSync
  events under partial replay). The two other opens here - the `update:` schema
  and whether `preferDevice` sets the bit - are now RESOLVED in the next
  subsection.

### The residency-config lever (RESOLVED: it is an env var)

The lever that
sets `0x276aaf289` bit 3 is an **environment variable**, read at controller
construction. No API call, no `preferDevice`, no `update:` message is needed.

- **The gate bit is byte `0x289` bit 3 = bit 11 of the word `0x276aaf288` = mask
  `0x800`** (MEASURED: reader `-[GTMTLReplayObjectMap addGlobalResidencySetToQueue:]`
  @ `0x24f8b6304` `ldrb w8,[x8,#0x289]; tbnz w8,#0x3`). The word `0x276aaf288`
  is field +0x30 of the global config struct `_GT_ENV` @ `0x276aaf258`.
  `-[GTMTLReplayObjectMap addToGlobalResidencySet:]` @ `0x24f8b62b8` reads the
  SAME bit, so bit 3 gates the entire residency path (both adding resources to
  the global set and binding the set to the queue) as one switch.

- **`_GTMTLReplayController_init` @ `0x24f8a0964` writes bit 11 from an env var,
  default 0** (MEASURED, `0x24f8a0bb8`-`0x24f8a0bd0`):
  `add x0,x0,#0xc2 ; "MTLREPLAYER_FORCE_RESOURCES_RESIDENT"`, `mov x1,#0`
  (default false), `bl _GetEnvDefault`, `bfi x8,x0,#0xb,#0x1`,
  `str x8,[x20,#0x30]` with `x20 = _GT_ENV`. So the reason the bit is clear is
  NOT "no writer runs" - init writes it on every bootstrap, defaulting to 0
  because the env var is unset. `_GetEnvDefault` @ `0x24f87cd24` is `getenv` +
  default, the standard `MTLREPLAYER_*` boolean shape (sibling bits in the same
  block are `MTLREPLAYER_FORCE_WAIT_UNTIL_COMPLETED`,
  `MTLREPLAYER_FORCE_BUFFER_STORAGE_MODE_PRIVATE`, etc.).

- **`preferDevice` does NOT set the gate.**
  `_GTMTLReplayClient_preferDevice` @ `0x24f7fe0b4` writes the flag word twice
  but both touch ONLY bit 21 (`0x200000`), never bit 11 (MEASURED,
  `0x24f7fe108`-`0x24f7fe158`). The open question "does preferDevice alone set
  the bit" is answered **NO**.

- **`update:` is a secondary route, moot.** `-[GTMTLReplayService update:]`
  block `0x24f80a574` also sets bit 11, from a boolean getter on its config
  object (`bl SUB_250003c60; csel w9,#0x800; str [x20,#0x288]` @
  `0x24f80a5c4`-`0x24f80a5e0`). The getter is a stub OUTSIDE the extracted image,
  so its selector name (hence the exact `update:` schema) is NOT statically
  recoverable - but it is unnecessary, since the env var reaches the same bit
  during init.

- **DO THIS BEFORE PLAYBACK:** set `MTLREPLAYER_FORCE_RESOURCES_RESIDENT=1` in
  the environment before bootstrap (before `GTMTLReplayController_init` runs; it
  reads the env at construction). This is the one statically-proven barrier;
  whether it is SUFFICIENT for the MTL4 buffer to complete in our headless
  setup, or whether a further gap remains behind the live `play_to(1)` hang,
  can only be settled by a live re-time.

### Live re-time WITH residency forced (2026-09-01): still does NOT complete

- Ran `MTLREPLAYER_FORCE_RESOURCES_RESIDENT=1 MTLCAPTURE_LOG_ERRORS=1
  MTLREPLAYER_STOP_ON_COMMIT_ERROR=1 probes/run.sh playback small.gputrace 2000
  playto:1`.
- Baseline sweep answered 4 of 2001 (fetch works, as always). `play_to(1)` then
  **busy-looped at 99% CPU** again (killed at ~54s; recovery clean, no lockout).
  A single command that would complete sub-second if fixed did not complete.
- CONCLUSION (MEASURED): `MTLREPLAYER_FORCE_RESOURCES_RESIDENT=1` is **necessary
  but NOT sufficient**. Enabling the residency-set code path (bit 3) does not, on
  its own, make the committed MTL4 buffer complete in our headless in-process
  setup. A further gap remains behind the GPU-only CoreSync fence.
- WHAT IS NOW KNOWN (the boundary): fetch commits+completes a real GPU buffer
  unentitled (so basic execution works); playback's MTL4 command buffers do not
  complete even with residency forced. Candidates for the residual gap, none
  established: (1) enabling bit 3 runs the "add residency set" path but the set
  is empty / the resources are not actually made resident (the flag gates the
  call, but the call still needs a populated residency set); (2) another config
  bit or a real `update:`/`preferDevice` step is also required (bit 3 alone is
  not the whole config the entitled path sends); (3) the MTL4 commit needs an
  explicit completion-driving step (a scheduled/committed callback pumped on a
  queue) our bare `playAll`/`playTo` C call does not provide; (4) the captured
  MTL4 command stream genuinely requires the full entitled GPUToolsDebugService
  pipeline to become executable. Distinguishing these needs either deeper static
  work (the addGlobalResidencySetToQueue path and what populates the set; the
  full config word the entitled path writes) or dynamic instrumentation, and is
  the open frontier for playback.

## Full config-word env-var map (STATIC)


`GTMTLReplayController_init` @ 0x24f8a0964 builds the entire replayer config
from env vars via `_GetEnvDefault`. Two shapes: a 64-bit bit-packed flag word
at `DAT_276aaf288` (= field +0x30 of `_GT_ENV`@0x276aaf258), and scalar/string
fields at `0x276aaf260..0x276aaf287`. MEASURED from the Ghidra decompile
(`scratchpad/re/configres.out:26-135`); the "gates" notes are MEASURED where an
address is cited, INFERRED from the name otherwise.

**38 bit flags** (bit: env var = default): 0 VALIDATE_LOAD_ACTIONS=0; **1
FORCE_WAIT_UNTIL_COMPLETED=0**; 2 FORCE_BUFFER_STORAGE_MODE_PRIVATE=0; **3
ENHANCED_COMMAND_BUFFER_ERRORS=0**; 4 DISABLE_OPTIMIZE_RESTORES=1; 5
METAL_FRAME_DEBUGGER_DISABLE_DISPLAY_ON_DEVICE=1; 6 DISABLE_PATCHING_ARRAYS=1; 7
PATCH_USING_ALL_RESOURCES=0; 8 ALLOW_BUFFER_PINNING=1; 9
ALLOW_PROGRAM_ADDRESS_TABLES=1; **10 FORCE_LOAD_UNUSED_RESOURCE=0 (LIVE
EFFECT MEASURED 2026-09-01: =1 makes every texture of a synthetic capture
fetchable, 0 -> 7; no effect on the two real captures; see dossier 00)**; **11
FORCE_RESOURCES_RESIDENT=0**; **12 IGNORE_UNUSED_RESOURCE=0 (LIVE EFFECT
MEASURED: =1 lets a capture load whose unused resources fail creation, and the
used ones then answer; dossier 00)**; 13
DISABLE_COMMAND_ENCODER_RESUME=0; 14 DISABLE_HEAP_TEXTURE_COMPRESSION=0; 15
DISABLE_SHADER_DEBUGGER_DRIVER_COMPILER_OPTIONS=0; **16 FORCE_FINISH_ON_REWIND=0**;
17 BUFFER_PINNING_REQUIRES_AB=1; 18 DISABLE_MEMORY_BARRIER_RENDER_TARGETS=0; 19
FORCE_DEFAULT_HAZARD_TRACKING=0; 20 FORCE_TRACKED_HAZARD_TRACKING=0; 21
ALLOW_OTHER_PLATFORMS=0; 22 DRAWABLE_RESOURCE_INDEX_WORKAROUND=0; 23
ALLOW_ALIAS_IOSURFACE_BACKED_BUFFERS=1; 24 GPURESOURCEID_SCAN_AND_PATCH=1; 25
LOAD_VALIDATION=0 (setenv MTL_DEBUG_LAYER=1); 26 LOAD_CAPTURE=0 (setenv
METAL_CAPTURE_ENABLED=1); 27 LOAD_HUD=0 (setenv MTL_HUD_ENABLED=1); **28
STOP_ON_COMMIT_ERROR=0**; 29 REDIRECT_LOGGING_TO_STREAMS=0; 30
LOCK_PARAM_BUFFER_SIZE_TO_MAX=1 (the existing unlock); 31 RESOURCES_ON_HEAPS=0;
32 LIVE_ICBS=0; 33 EXTRACT_FROM_HEAPS=0; 34 FORCE_FETCH_TEXTURES_FOR_DISPLAY=0;
**35 DISABLE_REPLAY_SERVICE=0**; 36 RESIZE_REBUILT_ACCELERATION_STRUCTURES=1; 37
DISABLE_AUTOMATIC_HEAPS=1. All names carry the `MTLREPLAYER_` prefix except bit
5 (`METAL_FRAME_DEBUGGER_...`).

**9 scalar/string fields:** ABORT_ON_ERROR_CODE (-1), ABORT_ON_FAILURE_TYPE (1),
FORCE_PATCHING_TYPE_REPLACE_MASK (0), ERROR_FILTERING (-1),
DISPLAY_FETCH_TIMEOUT_MS (2000), SHARED_RESOURCE_POOL_MAX_SIZE (0x80),
SLEEP_AFTER_RESTORE (0), RESTORE_THREAD_COUNT (nCPU-1), and the string
INSERT_BINARY_ARCHIVES.

**Completion-relevant highlights, and why none is a fix.** The stalling
`_GTMTLCoreSync_waitForMTL4CPU` is the FIRST act of
`_GTMTLReplay_commitCommandBuffer` (@0x24f7fd8e4), BEFORE the config word is
read (@0x24f7fd960); so no bit can skip it (MEASURED). `FORCE_WAIT_UNTIL_COMPLETED`
(bit 1) only ADDS an extra CPU wait in both commit paths
(`commitCommandBuffer` @0x24f7fd964; `commitMTL4CommandBuffers` @0x24f7fddac ->
`_GTMTLCoreSync_waitForValueCPU` @0x24f7fddc4) - worse, not better.
`STOP_ON_COMMIT_ERROR` (bit 28) already failed to stop the hang live. **VERDICT:
no env-var combination makes an un-completing buffer complete.** The only useful
live combo is diagnostic: `STOP_ON_COMMIT_ERROR=1` + `ENHANCED_COMMAND_BUFFER_ERRORS=1`
+ `ABORT_ON_ERROR_CODE=0` + `MTLCAPTURE_LOG_ERRORS=1`, to try to surface an error
rather than unblock.

## What populates the residency set (STATIC)

The global set is
ivar `_globalResourceResidencySet` at `GTMTLReplayObjectMap+0x280`
(`-globalResourceResidencySet` @0x24f8bef24 returns `*(self+0x280)`; MEASURED).

**Both required MTLResidencySet steps (commit + queue-bind) ARE wired under bit
11 - the wiring is complete, not missing.** (MEASURED, `ipsw macho disass`.)

- `-[GTMTLReplayObjectMap addToGlobalResidencySet:]` @ 0x24f8b62b4 (gated bit
  11, `tbnz byte0x289#3` else ret): `[globalSet addAllocation:arg]`
  (bl 0x2500018d0 @0x24f8b62dc) then tail-call `[globalSet commit]`
  (b 0x250002500 @0x24f8b62fc). So it adds an allocation AND commits it.
- `-[...addGlobalResidencySetToQueue:]` @ 0x24f8b6300 (and ...ToMTL4Queue:
  @0x24f8b6768): tail-call `[queue addResidencySet: globalSet]` (b 0x250001aa0).
- `makeController` @ 0x24f89813c, gated bit 11 (`tbz byte0x289#3` @0x24f89856c),
  binds a residency set to TWO queues at controller-setup time via
  `[queue addResidencySet:]` (0x250001aa0 @0x24f898598 and @0x24f8985d4) -
  INFERRED the MTL3 default + MTL4 queues. This is the bind step our bootstrap
  DOES run before playback.
- `AddTraceBuffersToResidencySet` @ 0x24f8ae0b0 is a self-contained create+add
  (x6)+commit+bind path, but its ONLY callers (MEASURED, `scratchpad/re/callerscan.out`)
  are `InstrumentFunctionWithResourceTrackingV2` @0x24f8af164 and
  `InstrumentSubCommandWithAccessTrackingV2` @0x24f8b04f8 - resource/access
  TRACKING instrumentation (profiler), OFF the plain play_to path.

**Conclusion.** Residency wiring under bit 11 is complete (commit present,
queue-bind present), yet the live play_to(1) still stalled with bit 11 forced.
So residency is confirmed necessary-not-sufficient by the code, and the residual
barrier is NOT residency but the upstream, unconditional
`_GTMTLCoreSync_waitForMTL4CPU` whose fence is never advanced by GPU completion
in the headless/unentitled/in-process setup. OPEN (untraceable here, auth-stub
islands 0x25000xxxx are absent from the extraction): who calls
`addToGlobalResidencySet:` in the play_to path, i.e. whether the bound global
set actually receives any allocations. Moot for the completion barrier, but
relevant if residency is pursued further.

## Status

- `GTMTLReplayController_playAll` - **Signature established (1 arg, controller)
  via static disassembly; semantics inferred ("play to end").** Graduated to
  callable FFI in `gputools-replay-sys::ffi`, declared returning `()`: it
  tail-calls `_GTMTLReplayController_debugSubCommandStop`, whose own return is
  not established, so no return value is consumed. **LIVE PROBE (2026-09-01,
  corrected): returns cleanly in about a second on `small.gputrace` when
  handed the client-struct controller. The earlier "does not return, ~99% CPU"
  result was our own bad controller pointer - see the CORRECTION at the top.**
  Whether the replay it performs is CORRECT is not established.
- `GTMTLReplayController_playTo` - **Signature established (2 args:
  controller, uint32 target command index) via static disassembly; forward-only
  replay-loop semantics established from control flow AND confirmed live.**
  Graduated to callable FFI in `gputools-replay-sys::ffi`, declared returning
  `()`: the exit path never deliberately sets a return value, so none is
  consumed. **LIVE PROBE (2026-09-01): `play_to(1)` returns cleanly and the
  command index at +0x5820 advances 0 -> 1 (`probes/run.sh playstep`).** It did
  not resolve the coverage gap; see the CORRECTION's open question on the
  post-playback reply parse.
- `GTMTLReplayController_rewind` - **Signature established (1 arg, controller)
  via static disassembly; semantics inferred (teardown + restore initial
  state) and confirmed live (2026-09-01): the command index returns to 0 after
  a `play_to(5)`.** Graduated to callable FFI in
  `gputools-replay-sys::ffi`, declared returning `()`: it passes through
  `_Rewind`'s own (unestablished) return value, so none is consumed.
