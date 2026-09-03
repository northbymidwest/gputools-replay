# Surface: shader profiler

Symbols:

- `GTShaderProfilerBinaryAnalysisResult` (ObjC class) - Unverified: unprobed.
- `GTShaderProfilerStreamData` (ObjC class) - Unverified: unprobed.
- `GTMutableShaderProfilerStreamData` (ObjC class) - Unverified: unprobed.
- `GTMTLReplayHost_generateDerivedDataPayload` (C symbol) - Unverified: arity
  unknown.

## Static findings

- **None of the three target classes has `-setStreamRef:` or
  `-setDispatchUID:`** (MEASURED, by absence, across the full transcribed
  method lists below). Every class examined in
  [00-texture-fetch.md](00-texture-fetch.md),
  [03-pipeline-fetch.md](03-pipeline-fetch.md), and
  [05-accel-structure.md](05-accel-structure.md) carries both. Their absence
  here is a clear structural break from the `GTReplayFetch*`/`GTReplayDecode*`
  request family, and confirms (previously only a "weak signal" from naming)
  that this surface is not dispatched the same `streamRef`-keyed way, or if
  it is, the key lives on some other, not-yet-identified class.

- **`GTShaderProfilerBinaryAnalysisResult`** (MEASURED,
  `classdump-27.txt:3797-3839`, instanceSize 328): shape is a decoded-analysis
  RESULT object, not a request. It has no `streamRef`/`dispatchUID` and no
  simple setters; instead it exposes several parallel table triples, each a
  read-only raw-pointer accessor (`r^{...}`, const pointer to an anonymous C
  struct), a matching `*Count` (`u64`) accessor, a `last*` accessor of the
  same pointer type, and a settable backing `NSData` (`@`) property:
  - `instructions` / `lastInstruction` / `instructionCount`, row type
    `r^{?=IIQQIICCS}` (u32,u32,u64,u64,u32,u32,u8,u8,u16); backed by
    `instructionData` (settable).
  - `clauses` / `lastClause` / `clauseCount`, row type `r^{?=QQIIII}`; backed
    by `clauseData` (settable).
  - `binaryRanges` / `lastBinaryRange` / `binaryRangeCount`, row type
    `r^{?=IQQII}`; backed by `binaryRangeData` (settable).
  - `binaryLocations` / `lastBinaryLocation` / `binaryLocationCount`, row
    type `r^{?=IIII}`; backed by `binaryLocationData` (settable).
  - `branchTargets` / `lastBranchTarget` / `branchTargetCount`, row type
    `r^{?=Q}`; backed by `branchTargetData` (settable).
  - `registerInfo` / `lastRegisterInfo` / `registerInfoCount`, row type
    `r^{?=II}`; backed by `registerInfoData` (settable).
  - `-stringAtIndex:` (`@24@0:8Q16`, `u64` index in, object out) plus
    `-setStrings:` (`@`): an index-addressed string table.
  - `-registerInfoOffsetForInstructionIndex:` (`Q24@0:8Q16`, `u64` in,
    `u64` out): a lookup method, confirming the object is queried after
    construction rather than filled field-by-field like a request.
  - `-version`/`-setVersion:` (`u32`), `-binaryInfo` (`^v`, raw void
    pointer, read-only, no setter), `-maxOffset` (`u64`).
  - `NSCoding` (`-initWithCoder:`/`-encodeWithCoder:`) is present, so it can
    round-trip through an archive as well as through the `*Data` setters.

- **`GTShaderProfilerStreamData`** (MEASURED, `classdump-27.txt:3874-3953`,
  instanceSize 344): shape is a large serializable container holding an
  entire recorded (or loaded) shader-profiling session. `NSCoding`
  (`-initWithCoder:`/`-encodeWithCoder:`) and `NSCopying` (`-copyWithZone:`)
  both present. Notable groups of members:
  - Identity/metadata: `traceName`, `metalDeviceName`, `metalPluginName`
    (all `@`, settable), `deviceInfo` (`@`, settable; presumably a
    `GTShaderProfilerDeviceInfo`), `unixTimestamp` (`i64`), `gpuGeneration`
    (`u32`), `version` (`u64`), `isPreSiData` (`BOOL`), `preSiBundleURL`
    (`@`), and an alternate constructor `-initWithPreSiBundle:` (`@24@0:8@16`)
    for loading from a "pre-Si" (pre-silicon / simulated-hardware) bundle
    instead of a live capture.
  - Profiling-mode mirror fields: `profiledExecutionMode`,
    `profiledPerformanceState`, `profiledProfilerMode` (all `u32`, all
    settable). These names exactly mirror
    `GTShaderProfilerSessionRequest`'s `executionMode`/`performanceState`/
    `profilerMode` (`classdump-27.txt:3862-3872`; not a scoped target for
    this task, cross-referenced only), a structural hint (not measured) that
    a `GTShaderProfilerStreamData` records the mode a session was actually
    profiled under, after being configured by a `GTShaderProfilerSessionRequest`.
  - Bulk data tables, same raw-pointer + count + backing-`@`-data pattern as
    `GTShaderProfilerBinaryAnalysisResult`: `pipelineStates` /
    `pipelineStateInfoData` / `pipelineStateInfoCount` (row
    `r^{?=QQQIIII}`); `encoders` / `encoderInfoData` / `encoderInfoCount`
    (row `r^{?=QQQIIII}`) plus `-encoderInfoFromFunctionIndex:` lookup;
    `commandBuffers` / `commandBufferInfoData` / `commandBufferInfoCount`
    (row `r^{?=QQQII}`); `gpuCommands` / `gpuCommandInfoData` /
    `gpuCommandInfoCount` (row `r^{?=IIIIQIi}`) plus
    `-GPUCommandInfoFromFunctionIndex:subCommandIndex:` (two-index lookup);
    `functionInfo` / `functionInfoData` / `functionInfoCount` (row
    `r^{?=QQQIIII[8c]}`, note a trailing inline 8-byte field, likely a
    fixed-size name/tag).
  - Six "archived"/"unarchived" data kinds, each an `@`-typed pair
    (`archived*`/`unarchived*`), with `-enumerateUnarchived*:` block methods
    for three of them only: `APSData` (archived + unarchived, no enumerate),
    `APSCounterData` (archived + unarchived, no enumerate), `APSTimelineData`
    (archived + unarchived, no enumerate), `BatchIdFilteredCounterData`
    (archived + unarchived + enumerate), `GPUTimelineData` (archived +
    unarchived + enumerate), `ShaderProfilerData` (archived only on this
    class; unarchived + enumerate present), `PerDrawRawCounterData`
    (archived only, no unarchived or enumerate accessor on this class). What
    "APS" expands to is not established (see Open questions).
  - Plain getters `batchIdFilterableCounters` (`@`) and
    `pipelinePerformanceStatistics` (`@`), and a paired
    `dataSourceHasUnusedResources` (`BOOL`) / `dataSourceCaptureRange`
    (`{_NSRange=QQ}`).
  - `-encode:error:` (`@32@0:8@16^@24`): a second serialize path distinct
    from `NSCoding`, taking an object argument and an `NSError**` out
    param. `-dataFromUnarchvedMetadata:` (`@24@0:8@16`, note: "Unarchved" is
    misspelled in the symbol itself, transcribed verbatim from
    `classdump-27.txt:3916`).
  - Local-file machinery: `-_setupDataPath`, `dataFileURL` (`@`),
    `-_writeLocalData:dataPath:to:`, `-cleanupLocalFiles`,
    `-debugDump:` (takes an object; cross-referencing `GTShaderProfilerDebugDump`
    at `classdump-27.txt:3841-3845`, which has `-initWithDirectory:`/
    `-setDirectory:`/`-filePathFromFileName:`, this plausibly takes a
    `GTShaderProfilerDebugDump` instance, not measured), `-patchObjectIds:`
    (`@24@0:8@16`, rewrites embedded object identifiers).
  - `strings` (`@`, read-only getter on this class; see
    `GTMutableShaderProfilerStreamData` for the mutator).
  - `supportsFileFormatV2` (`BOOL`, settable) plus a shared
    `-initWithNewFileFormatV2Support:` constructor present verbatim on both
    this class and `GTMutableShaderProfilerStreamData`
    (`classdump-27.txt:3900` and `:2513`), indicating a versioned on-disk
    file format both classes are aware of.
  This class is unambiguously a RESULT/data-container, not a request: it is
  far larger and richer than any `GTReplayFetch*` request (which top out at
  around ten simple setters), and every accessor either reads or holds a
  large, already-assembled data set rather than describing "what to fetch."

- **`GTMutableShaderProfilerStreamData` confirmed as the write/builder
  side** (MEASURED, `classdump-27.txt:2507-2533`, instanceSize 352): this
  moves the dossier's prior "consistent with a mutable-subclass pattern...
  but no method list has been transcribed to confirm" note to confirmed by
  method shape (the classdump does not record superclass names directly, so
  this is inferred from matching method shapes, not a declared `@interface
  ... : GTShaderProfilerStreamData`). Evidence:
  - `-addCommandBuffers:count:` (`v32@0:8^{?=QQQII}16Q24`),
    `-addEncoders:count:` (`^{?=QQQIIII}`), `-addGPUCommands:count:`
    (`^{?=IIIIQIi}`), `-addPipelineStates:count:` (`^{?=QQQIIII}`),
    `-addShaderFunctionInfo:count:` (`^{?=QQQIIII[8c]}`): each add* method's
    C-struct-array row encoding is byte-for-byte identical to the matching
    read accessor's row type on `GTShaderProfilerStreamData` above,
    confirming these are literally the appenders for those exact tables.
  - `-addAPSData:`, `-addAPSCounterData:`, `-addAPSTimelineData:`,
    `-addBatchIdFilteredCounterData:`, `-addGPUTimelineData:`,
    `-addShaderProfilerData:` (all `B24@0:8@16`: object in, `BOOL` out),
    paired with `-removeAPSData`, `-removeAPSCounterData`,
    `-removeAPSTimelineData` (`v16@0:8`, no args). Only three of the six
    archived kinds have a remove method; `BatchIdFilteredCounterData`,
    `GPUTimelineData`, and `ShaderProfilerData` have no remove counterpart in
    this dump.
  - `-addPipelinePerformanceStatisticsData:` (`v24@0:8@16`, void return,
    unlike the `BOOL`-returning adds above).
  - `-setNumBlitCalls:` (`v24@0:8Q16`, `u64`): setter for the base class's
    read-only `blitCallCount` (`classdump-27.txt:3911`).
  - `-setPerDrawRawCounterData:` and `-setBatchIdFilterableCounters:`
    (both `v24@0:8@16`): setters for base-class-only getters
    (`archivedPerDrawRawCounterData`, `batchIdFilterableCounters`).
  - `-setDataSourceHasUnusedResources:captureRange:`
    (`v36@0:8B16{_NSRange=QQ}20`): single combined setter for the base
    class's paired `dataSourceHasUnusedResources`/`dataSourceCaptureRange`
    getters.
  - `-addString:` (`Q24@0:8@16`, object in, `u64` out): mutator for the
    base class's read-only `strings` table.
  - `-_copyForAddAPSData:prefix:` (`@32@0:8@16@24`, two objects in, object
    out): suggests at least one add path produces a derived copy rather
    than mutating in place.
  - `-init`, `-.cxx_destruct`, `-_commonInit` are ordinary lifecycle
    methods; instanceSize is 352 vs. 344 on the base class (8 bytes larger),
    consistent with one extra ivar, but the classdump does not list ivars,
    so what that extra field holds remains unknown.

- **Cross-reference (not scoped targets, recorded for context):**
  `GTShaderProfilerSessionRequest` (MEASURED, `classdump-27.txt:3862-3872`,
  instanceSize 32) has `executionMode`/`performanceState`/`profilerMode`
  (all `u32`, settable) and `streamDataToLoad`/`setStreamDataToLoad:` (`@`,
  plausibly accepts a `GTShaderProfilerStreamData` to load previously
  captured data back for re-analysis, given the field name and the mirrored
  `profiled*` fields noted above; not measured). `GTReplayProfileRequest`
  (MEASURED, `classdump-27.txt:3331-3342`, instanceSize 40) is a further
  candidate entry point: `profileData`/`setProfileData:` (`@`),
  `streamHandler`/`setStreamHandler:` (`@?`, a block), `profileDataVersion`/
  `setProfileDataVersion:` (`i32`), `priority`/`setPriority:` (`u32`).
  Neither class was in this task's scope and neither has been read beyond
  this classdump transcription; whether one, both, or neither drives the
  shader-profiler surface toward producing a `GTShaderProfilerStreamData` is
  not established.

- `GTMTLReplayHost_generateDerivedDataPayload` remains absent from the class
  dump (MEASURED, full-file grep of all 335 class blocks in
  `classdump-27.txt` found no match): it is a C function, not an ObjC
  method, so an ObjC class dump cannot describe its signature. No new
  information beyond what this dossier already recorded.

## Open questions

- What dispatches a shader-profiling session and ultimately produces a
  `GTShaderProfilerStreamData` is narrowed to two named candidates
  (`GTShaderProfilerSessionRequest`, `GTReplayProfileRequest`, see Static
  findings) but neither was read live beyond this classdump transcription,
  and no confirmed call sequence exists connecting either to
  `GTMTLReplayService` or to producing a `GTShaderProfilerStreamData`.
- What "APS" stands for (Apple Performance Shaders? Application/GPU
  Performance Statistics?) is unknown; it appears throughout
  `GTShaderProfilerStreamData`'s and `GTMutableShaderProfilerStreamData`'s
  archived/unarchived accessor names but is never spelled out or defined in
  either class's own method list.
- **Possible required-field trap, unverified:** the raw-pointer (`r^{...}`)
  accessors on both `GTShaderProfilerBinaryAnalysisResult` and
  `GTShaderProfilerStreamData` read directly over a backing `NSData` set
  independently (e.g. `pipelineStates` over `pipelineStateInfoData`). Unlike
  `GTReplayFetchTexture`'s `depth` (a known required value of 1), there is no
  measurement here of what happens if a `*Data` property is unset, undersized
  relative to its paired `*Count`, or the `count:` argument to an `add*:`
  method on the mutable subclass exceeds the buffer it points at. This could
  plausibly crash rather than fail quietly, and should be treated as a live
  hazard to probe carefully, not assumed safe.
- Still fully open: whether this surface is reached via
  `-[GTMTLReplayService fetch:]` at all, or via a wholly separate
  service/class not yet identified. The confirmed absence of
  `setStreamRef:`/`setDispatchUID:` on all three target classes (see Static
  findings) is evidence against the simple "same as the Fetch family"
  hypothesis and should be weighted accordingly, but no alternative dispatch
  path has been confirmed either.
- What the extra 8 bytes of instance storage on
  `GTMutableShaderProfilerStreamData` (352 vs. 344 on the base class) hold:
  the classdump lists no ivars, only methods, so this cannot be answered
  from this source.
- Whether `GTMTLReplayHost_generateDerivedDataPayload` is a standalone
  operation or an argument to one of the `GTShaderProfiler*` classes, and
  what "derived data" refers to (a request input, a report output, or a
  cache artifact). No new evidence found in this pass.

## Live probes

None run yet.

## Status

- `GTShaderProfilerBinaryAnalysisResult` - **Shape established from the live
  runtime (2026-09-01)**, instanceSize 328: compiled-shader instruction
  analysis - `instructions` (`{?=IIQQIICCS}` per instruction), `instructionCount`,
  `clauses` (`{?=QQIIII}`), `binaryRangeData`/`binaryLocations` (`{?=IIII}`),
  `stringAtIndex:`, `version`. Role from the self-documenting selectors; not
  behavior-probed.
- `GTShaderProfilerStreamData` - **Shape established from the live runtime
  (2026-09-01)**, instanceSize 344: the READ side of a shader-profiler trace -
  `deviceInfo`, `version`, `strings`, `traceName`, `pipelineStates`, `encoders`
  (`{?=QQQIIII}`), `GPUCommandInfoFromFunctionIndex:subCommandIndex:`
  (`{?=IIIIQIi}`), `encode:error:`, NSCoding.
- `GTMutableShaderProfilerStreamData` - **Shape established from the live
  runtime (2026-09-01)**, instanceSize 352: the BUILDER side - `addString:`,
  `addCommandBuffers:count:`, `addEncoders:count:`, `addGPUCommands:count:`,
  `addPipelineStates:count:`, `addShaderFunctionInfo:count:` (`{?=QQQIIII[8c]}`),
  `addAPSData:`/`addAPSCounterData:`/`addAPSTimelineData:`,
  `addPipelinePerformanceStatisticsData:`. The `addPipelinePerformanceStatisticsData:`
  selector connects to the per-pipeline `PerformanceStatistics` seen in the
  live pipeline-binaries fetch (dossier 03).
- `GTMTLReplayHost_generateDerivedDataPayload` - **Signature established
  (2026-09-01) from the prologue (ipsw disass @ 0x24f88e270): 2 args** (`mov
  x21,x1; mov x23,x0`, no x2+ read as input). Generates the profiler
  derived-data payload; semantics inferred from the name and the profiler
  surface.
