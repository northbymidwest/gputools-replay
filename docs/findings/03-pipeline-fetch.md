# Surface: pipeline binaries fetch

Symbols:

- `GTReplayFetchPipelineBinaries` (ObjC class) - Unverified: unprobed.

## Static findings

- **Full method list transcribed** (MEASURED from `classdump-27.txt:2879-2886`):
  instanceSize 32. Methods, with decoded type encodings:
  - `-initWithCoder:` `@24@0:8@16` and `-encodeWithCoder:` `v24@0:8@16`:
    standard `NSCoding` pair, same shape as every other request class in this
    campaign.
  - `-setDispatchUID:` / `-dispatchUID`, encoding `(?={?=ii}Q)16`: a 16-byte
    anonymous *union* (parentheses, not braces) of `{int,int}` and `u64`. This
    exact encoding recurs verbatim on `GTReplayFetchTexture`,
    `GTReplayFetchThreadgroup`, `GTReplayDecodeAB`, and every other
    `GTReplayFetch*`/`GTReplayDecode*` class inspected in this pass, so it
    reads as a shared identifier field on a common (undisclosed by the dump)
    superclass, not something specific to pipeline binaries.
  - `-setStreamRef:` `v24@0:8Q16` / `-streamRef` `Q16@0:8`: a plain `u64`,
    same shape and offset as `GTReplayFetchTexture`'s `streamRef`
    (00-texture-fetch.md).
  - No other setters or getters are present.
- **This is the minimal member of the `GTReplayFetch*` family** (MEASURED, by
  comparison across `classdump-27.txt`): `streamRef` + `dispatchUID` and
  nothing else. Contrast with siblings that add an identifying field beyond
  the shared baseline: `GTReplayFetchBuffer` adds `-range` (`{GTRange=QQ}`,
  `classdump-27.txt:2783-2789`); `GTReplayFetchThreadgroup` adds `-index`
  (`u32`, `classdump-27.txt:2931-2937`); `GTReplayFetchTexture` adds
  `region`/`size`/`plane`/`depth`/`level`/`slice`/`resolveMultisampleTexture`
  (`classdump-27.txt:2909-2929`). `GTReplayFetchPipelineBinaries` has none of
  these extra fields, so as far as settable properties go, `streamRef` alone
  identifies the request.
- **Identical shape to the acceleration-structure fetch/decode classes**
  (MEASURED): `GTReplayFetchAccelerationStructure` and
  `GTReplayDecodeGenericAccelerationStructure`
  ([05-accel-structure.md](05-accel-structure.md)) have the exact same
  `streamRef` + `dispatchUID`, nothing-else shape at `classdump-27.txt:2775-2781`
  and `classdump-27.txt:2759-2765`. All three classes are structurally
  interchangeable by method list.
- **Batch/dispatch shape cross-reference** (MEASURED, `classdump-27.txt:3423-3434`):
  `GTReplayRequestBatch` exposes `-setRequests:` (`@`, NSArray),
  `-setCompletionHandler:` (`@?`, block), `-requestID`/`-setRequestID:` (`u64`),
  `-priority`/`-setPriority:` (`u32`), and `-initNoRequestID`. This is the same
  batch shape already used for `GTReplayFetchTexture` (HANDOFF 2.4), and
  `GTReplayFetchPipelineBinaries`'s `NSCoding` conformance is consistent with
  being placed in the same `-setRequests:` array, but that dispatch path is
  not itself confirmed for this class (see Open questions).
- **`GTReplayUnarchiver` has an empty instance-method list** (MEASURED,
  `classdump-27.txt:3564`): instanceSize 8 (an isa pointer only, no ivars),
  and the dump lists zero instance methods for it. Either its behavior lives
  entirely in class methods (which an instance-method dump does not capture)
  or it inherits everything from an undisclosed superclass. This is a new,
  concrete fact: the class is real and present, but this pass's source
  material provides no evidence at all about how it operates, not even a
  partial method list to reason from.

## Open questions

- No behavior has been established for `-setDispatchUID:`/`-dispatchUID`
  anywhere in this family: whether it must be set to a particular value,
  whether it is caller-assigned or framework-assigned, and what happens if it
  is left at its default. Unlike `GTReplayFetchTexture`'s `depth` (which has a
  known required value of 1), there is no measurement at all here, only the
  fact that the setter exists.
- Whether `GTReplayFetchPipelineBinaries` is dispatched through the same
  `-[GTMTLReplayService fetch:]` entry point as texture fetch, or a different
  one. The shared `NSCoding` + batch-compatible shape is a structural
  inference, not a confirmed fact about this specific class's dispatch.
- What identifies a pipeline binary request beyond `streamRef`: whether one
  `streamRef` maps to one complete pipeline-binaries blob (all binaries for
  that pipeline state) with no further addressing, or whether the reply
  itself is what disambiguates. Nothing in the class shape hints at a
  sub-item selector (contrast `GTReplayFetchTexture`'s `plane`/`level`/`slice`).
- The reply format for this fetch kind: whether it shares the
  `unknown`/`info`/`data` three-key `NSKeyedArchiver` shape documented for
  texture fetch in [00-texture-fetch.md](00-texture-fetch.md), or has its own.
- Whether `GTReplayUnarchiver` decodes this reply at all: its empty
  instance-method list (see Static findings) means this pass could not even
  form a hypothesis about it, let alone test one. A live probe with
  `class_copyMethodList(cls, &count)` passing `NO` for `isInstanceMethod`
  would be the next step, not attempted here per the sessionless-only scope
  of this task.

## Live probes

None run yet.

## Live findings (2026-09-01)

`GTReplayFetchPipelineBinaries` was fetched live on `corpus.gputrace`
(35 render + 1 compute pipeline per `gpudebug`) through the generic
`Session::fetch_raw` path (`probes/run.sh rawfetch`), which builds the request
with `-setStreamRef:` only - no `dispatchUID`, no texture setters - and reuses
the established `GTReplayRequestBatch` + `-fetch:` machinery. MEASURED:

- **Same reply envelope as textures.** The response `-data` is an
  `NSKeyedArchiver` `bplist00` whose payload dict has the exact three keys
  `unknown` / `info` / `data`, so `reply::parse_reply` decodes it unchanged.
- **Same 80-byte `info` record stride** as the texture table, with the same
  accumulating `data_offset` at `0x18` and `size` at `0x1c` (record 0: off 0,
  size 36758; record 1: off 36758, size 567625; record 2: off 603871 = the
  running sum). But the identity fields DIFFER from textures: `0x00` is a
  sequential pipeline id (1184, 1185, ...), `0x08` is a 64-bit pipeline
  handle/address (e.g. `0x78f59d3000`). The texture layout's `0x08 == streamRef`
  does NOT hold here, so a pipeline reply must not be read with the texture
  field names.
- **The requested streamRef is a command-stream THRESHOLD, not a resource key**
  (established by single-ref fetches). Requesting ref 1183 returns 1 pipeline,
  1184 returns 2, 1500 returns 13, 2000 returns 13 - the reply is the cumulative
  set of pipeline binaries used up to that point, consistent with pipelines
  being created lazily as the command stream references them. This is unlike
  the per-resource texture fetch, and matches why `gpudebug` shows pipelines
  with an `info` action but no per-resource `fetch`.
- **Each record's `data` payload is itself a nested `bplist00`** with keys
  `fragment`, `vertex`, `fragment-dynamic-libraries`, `vertex-dynamic-libraries`,
  and `PerformanceStatistics`. The `vertex`/`fragment` blobs are compiled GPU
  binaries in Mach-O containers (magic `cf fa ed fe`); `PerformanceStatistics`
  carries per-stage `uniqueId` and counters ("FP16 instruction count", "Wait
  instruction count"). So the fetch yields the actual compiled shader machine
  code plus its statistics, per pipeline.

**The nested payload decoded** (2026-09-01): each pipeline record's `data` is
an `NSKeyedArchiver` `bplist` with five keys:

- `vertex`, `fragment` - each an array of one dict `{data: <Mach-O GPU binary>,
  uniqueId: N}`. The `data` blob is a compiled shader in a Mach-O container
  (magic `cf fa ed fe`); `uniqueId` is the per-function id.
- `vertex-dynamic-libraries`, `fragment-dynamic-libraries` - arrays of the
  stage's dynamic libraries (empty for a self-contained shader).
- `PerformanceStatistics` - a dict `{Fragment Shader: {...}, Vertex Shader:
  {...}}` of per-stage AGC-compiler metrics: instruction counts (FP16, FP32,
  INT16, INT32, ALU, Branch, Texture reads/writes, Device load/store, Wait),
  register counts, threadgroup memory, compilation time in ms, and the raw LLVM
  optimization remarks (YAML `--- !Passed`/`!Analysis`/`!Missed` from
  `GPUCompiler.framework` `agc.main`).

So `GTReplayFetchPipelineBinaries` yields, per pipeline, the compiled GPU
machine code for each stage plus the compiler's full statistics and remarks.

**COMPUTE pipeline shape + archiver note (2026-09-02, from `gputools-replay-hl`
Task 5, ground-truthed on `known-buffers.gputrace`).** Two refinements to the
above, both MEASURED while building the safe decoder:
- The nested `data` payload is a FULL `NSKeyedArchiver` archive (`$objects`/
  `$top` with UID indirection), not a plain `bplist` - `plist::from_bytes` alone
  does not unarchive it; a UID-resolving walk (the same shape as the outer reply
  envelope) is required. The `vertex`/`fragment`/`compute` stage entries are
  `NSArray`s (of one `{data, uniqueId}` dict each), which is why they appear as
  "an array of one dict" above.
- For a COMPUTE pipeline the stage key is `compute` (confirmed), and
  `PerformanceStatistics` is an `NSArray` of per-stage stat dicts with NO
  stage-name keys - it is NOT the `{Fragment Shader, Vertex Shader}` name-keyed
  DICT the render-pipeline read (2026-09-01) above describes. So the
  PerformanceStatistics container shape DIFFERS between render (name-keyed dict)
  and compute (unnamed array); the safe crate's `Pipeline::performance_stats`
  decodes the single-entry compute shape and the multi-stage/render shape stays
  unverified against a real render-pipeline fixture. Reconciling the two (are
  they genuinely different, or the same graph read at different unarchiving
  depths?) is an open follow-up.

Not yet done: confirming the `0x00`/`0x08` identity fields by cross-referencing
`gpudebug`'s pipeline listing; decoding the Mach-O GPU binary itself.

## Status

- `GTReplayFetchPipelineBinaries` - **Behavior confirmed (2026-09-01):
  fetched live, reply format fully characterized (see Live findings above).
  Signature established from instanceSize 32; the settable request shape is
  `-setStreamRef:` (`Q`, u64) and `-setDispatchUID:` (`(?={?=ii}Q)`), with the
  NSSecureCoding `-initWithCoder:`/`-encodeWithCoder:` pair - byte-for-byte the
  same base as the established `GTReplayFetchTexture` (which is a superset). So
  a pipeline-binaries fetch is built exactly like a texture fetch, minus the
  texture-only region/level/plane setters. Pinned by
  `inventory::tests::the_fetch_request_classes_share_the_streamref_shape`. A
  LIVE fetch is not yet run; `corpus.gputrace` has 35 render pipelines
  and 1 compute pipeline (per `gpudebug`), so it is the capture to try it on.
