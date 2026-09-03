# Surface: infrastructure

Symbols:

- `GTMTLReplayClient_createNewTransport` (C symbol) - Unverified: arity
  unknown; transport surface.
- `GTMTLReplayClient_destroyNewTransport` (C symbol) - Unverified: arity
  unknown; transport surface, presumed paired with
  `GTMTLReplayClient_createNewTransport`.
- `GTMTLReplayClient_preferDevice` (C symbol) - **Signature established; live
  audit (2026-09-01) = DEAD END in-process.** `void
  GTMTLReplayClient_preferDevice(GTMTLReplayClient *client)`, a device-SELECTION
  routine (see the signature section below). Called live on a fully loaded
  in-process client, it FAULTS: `sample` shows `preferDevice+80` ->
  `HandleCrashSignal` recursion (the same crash-handler recursion as the
  original controller-pointer bug). ROOT CAUSE, measured directly
  (`probes/run.sh clientfields`): on the `-initWithContext:` client the fields
  are `[client+0x00]`=pool, `[client+0x08]`=controller, `[client+0x60]`=a
  pointer, but **`[client+0x30]` = 0x0**. `preferDevice` feeds `[client+0x30]`
  to `_GTSMMTLContext_getObject`, which dereferences the null and faults. So the
  two prior segfaults (HANDOFF 3) were never a wrong argument - `preferDevice`
  needs a client-state field that only the transport-path client setup
  (`createNewTransport`) populates, not `-initWithContext:`. There is no
  in-process way to call it safely; it is a dead end on this path.

  OUT-OF-PROCESS attempt (2026-09-01, `probes/run.sh transport`): the transport
  path is exactly what populates `[client+0x30]` - `createNewTransport(client)`
  opens the NSXPC connection to `com.apple.gputools.replay` and its proxies fill
  those fields. But calling `createNewTransport` from our process CRASHES: it
  SIGSEGVs inside its OWN `apr_pool_create_ex` (crash report:
  `apr_pool_create_ex+72 <- createNewTransport+76`), even though this process's
  own `apr_pool_create_ex` succeeds immediately before. So some precondition
  `createNewTransport` needs is not satisfiable from our unentitled code, and it
  never reaches the XPC connection. The transport path itself is healthy -
  `gpudebug` drives it and replays fine on this machine - but `gpudebug` is
  Apple-signed with the `com.apple.gputools.replay` entitlement and we are not,
  and it is SIP-protected so we cannot attach to it either. CONCLUSION: there is
  no way to RUN `preferDevice` from our own code, in-process or out. It does not
  matter for the campaign: `preferDevice`'s device-selection behaviour is already
  fully established statically (the signature section below), so a live call
  would only confirm what is known. It stays a resolved dead end.

  CAN WE ENTITLE OUR PROCESS? Tested 2026-09-01 - NO. `gpudebug`'s entitlement
  (`codesign -d --entitlements`) is `com.apple.private.gputools.client = true`
  (plus `platform-application` and a temporary-sandbox profile); it is signed by
  Apple's "macOS Software Signing" as a platform binary. `com.apple.private.*`
  entitlements are RESTRICTED: AMFI only honors them on an Apple-platform-signed
  binary. Self-signing proves it: `codesign -f -s - --entitlements` will put the
  entitlement string into our binary's signature, but running it is SIGKILLed at
  exec (exit 137) because the ad-hoc signature is not authorised to carry a
  restricted private entitlement. The only bypass is disabling AMFI system-wide
  (`amfi_get_out_of_my_way=1`, which needs SIP off via recovery + a reboot) - a
  drastic, insecure change that is out of scope, and `createNewTransport` would
  still hit the separate apr crash above. So there is no practical way to entitle
  our process; the entitled path belongs to `gpudebug` alone.
- `GTMTLReplay_CLI` (C symbol) - **Signature established (2026-09-01, ipsw
  disass @ 0x24f8c5ba4): 3 args** (x0 stored to stack, x1->x22, x2->x20). A CLI
  entry point (argc/argv-shaped); role inferred.
- `GTMTLReplay_fillError` (C symbol) - **Signature established (2026-09-01, @
  0x24f8918d8): 3 args** - an out `NSError**` (x0, `cbz`-guarded), then x1/x2
  forwarded to `_MakeNSError`, whose result is stored at `*x0`. An error-filling
  helper.
- `GTMTLReplay_handleError` (C symbol) - **Signature established (2026-09-01, @
  0x24f890bd4): 6 args** (x0..x5 all saved to callee-saved registers before
  use). An error handler; role inferred.
- `GT_ENV` (C symbol) - Unverified: likely data, not code; an env-var table
  worth inspecting.
- `GTGPUAPSConfig` (ObjC class) - Unverified: role unknown.
- `GTMTLTextureRenderer` (ObjC class) - Unverified: role unknown.
- `GTMTLTextureRenderEncoder` (ObjC class) - Unverified: role unknown.
- `GTTransportMessage_replayer` (ObjC class) - Unverified: transport surface.

## Static findings

- `GTGPUAPSConfig` appears in `docs/findings/raw/classdump-27.txt` with
  instanceSize 200.
- `GTMTLTextureRenderer` appears with instanceSize 64;
  `GTMTLTextureRenderEncoder` appears with instanceSize 32. The dump also
  lists `GTMTLTextureRenderEncoderCommand` (instanceSize 224), not in
  `inventory::EXPORTS`, which is likely a companion type used by
  `GTMTLTextureRenderEncoder` given the shared prefix.
- `GTTransportMessage_replayer` appears with instanceSize 72. The dump also
  lists `GTBaseSocketTransport_replayer` (instanceSize 264) and
  `GTTransportMessageReplyContinuation_replayer` (instanceSize 48), both
  sharing the `_replayer` suffix and not in `inventory::EXPORTS`; together
  they are the strongest available hint at how the transport pair
  (`GTMTLReplayClient_createNewTransport` /
  `GTMTLReplayClient_destroyNewTransport`) and `GTTransportMessage_replayer`
  relate: a socket-based transport carrying serialized transport messages,
  independent of (or alongside) the direct in-process `-fetch:` path used by
  texture fetch ([00-texture-fetch.md](00-texture-fetch.md)).
- `GTMTLReplayClient_preferDevice`'s two segfaulting argument candidates are
  recorded in `docs/HANDOFF.md` section 3 as an open item carried forward
  from the prior project; neither candidate is named here because neither
  produced a usable result, only a crash, and there is no third candidate
  proposed yet.
- `GT_ENV`, `GTMTLReplay_CLI`, `GTMTLReplay_fillError`, and
  `GTMTLReplay_handleError` are C symbols; none appears in the ObjC class
  dump. No disassembly has been done on any of the four.

## Transport / service driving architecture (RESOLVED, static)

`GTMTLReplayClient_createNewTransport` is **not** a socket transport for
in-process fetch. It is the **NSXPC client** side of the entitled replay server.

- `_GTMTLReplayClient_createNewTransport` (FW `0x24f8d5114`): creates an APR pool
  + client (`GTMTLReplayClient_init`), opens an NSXPC connection to the mach
  service string `com.apple.gputools.replay` (`0x24fcdb000+0xd8a`), and wires up
  remote-object proxies for the protocols `GTBulkDataService` and
  `GTMTLReplayService` (protocol refs at `0x2716a7000+0xbb8`/`+0xbc0`). So a
  caller of `createNewTransport` becomes a CLIENT of an out-of-process,
  entitled `GTMTLReplayService`; it does not drive replay itself.
- The `GPUToolsReplayService.xpc` binary that links these symbols is a near-stub
  broker: its `start` sets up a controller + `createNewTransport` then calls
  `xpc_main` with a handler (`0x100000a5c`) that is a single
  `b _xpc_connection_cancel` (rejects all inbound connections). It also maps
  GRAPHICS-ledger shared memory (`mach_make_memory_entry_64`,
  `mach_memory_entry_ownership`, `vm_map`; strings `com.apple.gputools.transport`,
  `failed to mark memory(GRAPHICS)`).
- The real replay engine is statically linked into the sibling
  `GPUToolsDebugService.xpc` (20 MB, hundreds of defined `_GTMTL*` symbols, links
  Metal). Both are launched/brokered by `/usr/libexec/gputoolsserviced`
  (`/System/Library/LaunchAgents/com.apple.gputoolsserviced.plist`, vends
  `com.apple.gputools.service`, `LimitLoadToDeveloperMode = true`).
- In-process, our crate uses `-[GTMTLReplayService initWithContext:]` and the
  local ObjC class directly (see `probes/src/session.rs`). The transport path
  uses `-initWithService:properties:bulkDataService:bulkDataServiceProperties:`,
  i.e. it is additionally wired with a `GTBulkDataService` (bulk resource-content
  channel) that the in-process path lacks.
- Relevance to playback: driving replay via the ObjC service methods
  (`update:`/`display:`/`profile:`) is how the config flags at `0x276aaf288` get
  set, which gate MTL4 residency-set attachment. Our bare `playAll` path skips
  them. See [01-playback.md](01-playback.md) "Service vs in-process diff".

## GTMTLReplayClient_preferDevice signature

**Established (static, function at 0x24f7fe0b4):**
`void GTMTLReplayClient_preferDevice(GTMTLReplayClient *client)` -- exactly ONE
argument, the client/self pointer.

- 0x24f7fe0d8 `mov x23, x0` is the only use of an incoming argument register.
  x1 is first written by `mov w1, #0x1` (0x24f7fe0fc), x2 is first loaded from
  `[x23,#0x60]` (0x24f7fe0f8); neither is read as an incoming argument, so the
  arity is 1.
- The body dereferences `client` fields `[x23]`, `[x23,#0x30]`, `[x23,#0x60]`
  (the last two fed to `_GTSMMTLContext_getObject`, 0x24f7fe0f4-100). These
  must already be populated. Calling `preferDevice` on a wrong or too-early
  object dereferences junk; that (not a wrong second-argument TYPE) is the most
  likely cause of the two prior segfaulting candidates recorded in
  `docs/HANDOFF.md` section 3. There is no second argument to get wrong.

The device to prefer is resolved INTERNALLY, not passed in:
- env `MTLREPLAYER_OVERRIDE_DEVICE_REGISTRY_ID` (string 0x24f7fe1e8, getenv-shape
  call 0x250015a00): if set, parse to a uint64 and resolve a device, then
  enumerate all devices matching on registryID (0x25000bdf0 / 0x250006b10,
  loop 0x24f7fe2a8) and `_EnableDeviceConfiguration` on the match.
- a preferred-device NAME path (internal descriptor `x21` non-null,
  0x24f7fe318): builds an enumerator block and calls a device-enumerate thunk
  (0x2500038a0), scoring each device by name. The block
  (`___GTMTLReplayClient_preferDevice_block_invoke`, 0x24f7fe578) calls
  `_GTNameSimilarityScore(deviceName, capturedTargetName)` at 0x24f7fe5a8 and
  keeps the highest-scoring device. PROVEN block signature from the descriptor
  symbol `___block_descriptor_56_..._v32?0"<MTLDevice>"8Q16^B24`, whose invoke
  type decodes to `void (^)(id<MTLDevice> device, unsigned long long index,
  BOOL *stop)`.
- env / config `MTLOverrideDeviceCreationFlags` (string 0x24f7fe240): a hook
  invoked via `blraaz` (0x24f7fe258) when present.

So `preferDevice` selects among the real devices already known to the client;
it cannot be handed an `id<MTLDevice>`. To bind a specific device
deterministically, set `MTLREPLAYER_OVERRIDE_DEVICE_REGISTRY_ID` and call
`preferDevice(client)` (single arg) after `GTMTLReplayClient_init`.

## Execution-gating environment variables

Full uppercase env-var inventory dumped via `strings`. Beyond the known
`MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX` unlock, the execution-relevant ones:

Device selection:
- `MTLREPLAYER_OVERRIDE_DEVICE_REGISTRY_ID` -- force the replay device by
  registryID (read in `preferDevice`, 0x24f7fe1e8). Strongest device knob.
- `MTLOverrideDeviceCreationFlags` -- device creation flags (0x24f7fe240).

Wait / completion cluster (candidates gating the CPU fence wait; names proven
present, but the replay-commit call sites are partly out-of-slice):
`MTLREPLAYER_FORCE_WAIT_UNTIL_COMPLETED`,
`MTLCAPTURE_FORCE_WAIT_UNTIL_COMPLETED_ON_COMMIT`,
`MTLCAPTURE_WAIT_SHARED_EVENT_TIMEOUT_CPU`,
`MTLCAPTURE_FORCE_WAIT_SHARED_EVENT_TIMEOUT_CPU`,
`MTLCAPTURE_WAIT_EVENT_TIMEOUT`, `MTLCAPTURE_WAIT_FOR_SIGNAL`,
`MTLREPLAYER_FORCE_FINISH_ON_REWIND`, `MTLREPLAYER_STOP_ON_COMMIT_ERROR`,
`MTLCAPTURE_LOG_ERRORS`, `MTLCAPTURE_GPU_RESTART_DEBUGGING`,
`MTLREPLAYER_DISABLE_REPLAY_SERVICE`.

NUANCE: the per-commit wait timeout in `_GTMTLReplay_commitCommandBuffer` is a
HARDCODED constant `0x1f40` (=8000), `mov w1, #0x1f40` at 0x24f7fd8e0 -- not
read from env in that path. The `MTLCAPTURE_WAIT_*` vars are the capture-side
analogs; none was proven to override this replay-commit constant. See
[01-playback.md](01-playback.md) "## Can playback execute in-process?".

## Open questions

- RESOLVED (see "Transport / service driving architecture" above): the transport
  pair is the NSXPC client side of the entitled out-of-process
  `GTMTLReplayService` (mach service `com.apple.gputools.replay`), not a socket
  fetch path. It is how Xcode/gpudebug reach the entitled replay server; the
  in-process `-fetch:` path does not need it. Still open: whether wiring a
  `GTBulkDataService` (as the transport init does) matters for any in-process
  operation, or is purely for streaming resource contents across the XPC boundary.
- `GTMTLReplayClient_preferDevice`'s argument type is now RESOLVED: it takes a
  single argument, the client (see the signature section above). The remaining
  question is operational, not the signature: at what lifecycle point the
  client fields it dereferences (`[client+0x30]`, `[client+0x60]`) are valid,
  and whether the two prior segfaults were purely a too-early / wrong-object
  call under the single-arg understanding.
- Whether `GTMTLReplay_CLI` is an entry point analogous to a `main`-like
  dispatcher for the `gpudebug` CLI, purely internal, or something callable
  usefully from a library.
- What `GTMTLReplay_fillError` and `GTMTLReplay_handleError` do relative to
  the already-established error-observer path
  (`GTMTLReplayErrorHandling_initWithObserver`, HANDOFF 2.2): whether they are
  the internals behind that observer callback, a separate error channel, or
  unrelated.
- Whether `GT_ENV` is genuinely a data symbol (a table of recognized
  environment variables, of which `MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX`,
  HANDOFF 2.1, would be one entry) versus a function; nothing has confirmed
  either reading, it is only a naming guess ("likely data, not code" per
  `inventory.rs`).
- What role `GTGPUAPSConfig`, `GTMTLTextureRenderer`, and
  `GTMTLTextureRenderEncoder` play at all; no hypothesis stronger than "named
  suggestively" exists for any of the three yet.

## Live probes

None run yet.

## Status

- `GTMTLReplayClient_createNewTransport` - **Signature established (2026-09-01,
  ipsw @ 0x24f8d5144): 1 arg** (`mov x23,x0`, the client). Opens the NSXPC
  transport to the entitled service (the obsolete path, handoff 5F); the
  signature is a fact regardless of the path's value.
- `GTMTLReplayClient_destroyNewTransport` - **Signature established (2026-09-01,
  @ 0x24f8d5518): 0 args** - it releases and nulls the singleton `_connection`
  global at `0x276aae000+0xf40` and reads no incoming argument register.
- `GTMTLReplayClient_preferDevice` - **Signature established (static): one
  argument, the client.** Behavior on a live client is still unprobed. Prior
  segfaults are consistent with a too-early / wrong-object call, not a wrong
  argument type.
- `GTMTLReplay_CLI` - **Unverified.** No probe attempted.
- `GTMTLReplay_fillError` - **Unverified.** No probe attempted.
- `GTMTLReplay_handleError` - **Unverified.** No probe attempted.
- `GT_ENV` - **Established (2026-09-01): the framework's global config/env
  table.** A DATA export (declared `pub static GT_ENV` in the sys crate). LIVE
  (`probes/run.sh gtenv`): the config word at `GT_ENV+0x30` reads 0x0 before
  `GTMTLReplayController_init` and 0x3001820b70 after, with bit 11 (0x800) set
  from `MTLREPLAYER_FORCE_RESOURCES_RESIDENT=1`. So `init` populates this word
  from the `MTLREPLAYER_*` env vars (one `bfi`-ed bit each, dossier 01), and
  `GT_ENV` is the base of that config global (runtime @ 0x27a3ab258 this run;
  statically `_GT_ENV` @ 0x276aaf258, config word `_GT_ENV+0x30` = 0x276aaf288).
- `GTGPUAPSConfig` - **Shape established from the live runtime (2026-09-01).**
  instanceSize 200; a GPU performance/tracing config object: `-duration`,
  `-pulsePeriod`, `-tileTracing`, `-countPeriod`, `-eslInstTracing`,
  `-cliqueTraceLevel`, `-emitThreadControlFlow`, `-bufferSizeInKb`,
  `-toDictionary`, `-initForTimeline`/`-initForCounters`. Belongs to the
  shader-profiler / GPU-counter surface (dossier 04). Role from selectors; not
  behavior-probed.
- `GTMTLTextureRenderer` - **Shape established from the live runtime
  (2026-09-01).** instanceSize 64; the texture-to-view PREVIEW renderer:
  `-initWithDevice:`, `-render:withEncoder:withFormat:renderTargetSize:viewContentsScale:`,
  `-renderTexture:isDepthStencil:shrinkToFit:withEncoder:...transform:anchor:bounds:...`
  (CATransform3D/CGRect/CGPoint), `-renderOverlay:...`. This is the GPU-tools
  texture-viewer renderer, not part of the fetch reply path. Role from the
  self-documenting selectors; not behavior-probed.
- `GTMTLTextureRenderEncoder` - **Shape established from the live runtime
  (2026-09-01).** instanceSize 32; the command encoder paired with
  `GTMTLTextureRenderer`: `-drawTexture:isDepthStencil:shrinkToFit:`,
  `-drawOverlay:color:shrinkToFit:`, `-setTransform:`, `-setBounds:contentsScale:`,
  `-setWaitForEvent:value:`, `-submitCommand`.
- `GTTransportMessage_replayer` - **Shape established (2026-09-01) from the live
  runtime**, instanceSize 72: an XPC transport message - `transport`, `payload`,
  `kind`, `serial`, `attributes`, `attributeForKey:`, `boolForKey:`,
  `doubleForKey:`.
