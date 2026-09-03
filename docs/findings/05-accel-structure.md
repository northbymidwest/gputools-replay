# Surface: acceleration structure

Symbols:

- `GTReplayFetchAccelerationStructure` (ObjC class) - Unverified: unprobed;
  the one export new in the macOS 27 SDK versus 26.
- `GTReplayDecodeGenericAccelerationStructure` (ObjC class) - Unverified:
  unprobed.

## Static findings

- **Full method lists transcribed** (MEASURED). Both classes have
  instanceSize 32 and, byte-for-byte, the identical method shape:
  - `GTReplayFetchAccelerationStructure` (`classdump-27.txt:2775-2781`):
    `-initWithCoder:` `@24@0:8@16`, `-encodeWithCoder:` `v24@0:8@16`,
    `-setDispatchUID:`/`-dispatchUID` `(?={?=ii}Q)16` (shared union type, see
    [03-pipeline-fetch.md](03-pipeline-fetch.md)), `-setStreamRef:`
    `v24@0:8Q16` / `-streamRef` `Q16@0:8` (`u64`). No other members.
  - `GTReplayDecodeGenericAccelerationStructure` (`classdump-27.txt:2759-2765`):
    `-initWithCoder:`, `-encodeWithCoder:`, `-setDispatchUID:`/`-dispatchUID`,
    `-setStreamRef:`/`-streamRef`, with the exact same encodings as above. No
    other members.
- **`GTReplayDecodeGenericAccelerationStructure`'s shape does not look like a
  decoder** (MEASURED, by absence): it has no data-holding property, no
  buffer/`NSData` ivar exposed through accessors, and no decode method beyond
  the standard `NSCoding` pair. Its only configurable input is `streamRef`,
  identical to a `Fetch` request. This contradicts this dossier's prior
  working hypothesis ("a decode step applied to whatever
  `GTReplayFetchAccelerationStructure` returns, analogous to how this
  crate's `reply` module decodes..."): nothing in the class shape supports a
  client-side two-step pipeline where a `Fetch` reply is handed to a
  `Decode` object. The prior hypothesis should be treated as superseded by
  this measurement, not merely unconfirmed.
- **Cross-reference: the wider `GTReplayDecode*` family is not uniform**
  (MEASURED). `GTReplayDecodeICB` (`classdump-27.txt:2767-2773`) shares the
  exact same minimal `streamRef` + `dispatchUID` shape. `GTReplayDecodeAB`
  (`classdump-27.txt:2749-2757`) additionally has `-setIndex:`/`-index`
  (`u32`) and `-setType:`/`-type` (`u16`). So within this naming family,
  some `Decode*` requests are `streamRef`-only (ICB, GenericAccelerationStructure)
  and at least one (AB) needs an extra sub-index/type selector. The working
  hypothesis this suggests, not established: `Fetch` and `Decode` are two
  different REQUEST KINDS dispatched for the same `streamRef` (raw bytes vs.
  an interpreted/decoded structure), submitted the same way (allocate,
  set fields, put in a `GTReplayRequestBatch`), rather than a fetch-then-decode
  client pipeline. This needs a live probe or fixture to test even
  provisionally; it is not measured.
- **`GTReplayFetchAccelerationStructure` follows the same `GTReplayFetch*`
  shape as its siblings** (MEASURED): identical to
  `GTReplayFetchPipelineBinaries` ([03-pipeline-fetch.md](03-pipeline-fetch.md))
  in every respect (instanceSize 32, same two settable fields), which is a
  stronger structural match than the naming-convention-only inference this
  dossier previously recorded.
- **A related but distinct class exists for pushing, not fetching,
  acceleration-structure data** (MEASURED, `classdump-27.txt:3566-3573`):
  `GTReplayUpdateAccelerationStructureSession` (instanceSize 32) has
  `-setData:`/`-data` (`@`, arbitrary object payload) and
  `-setSessionsID:`/`-sessionsID` (`u64`), keyed by `sessionsID` rather than
  `streamRef`. This was not one of the classes scoped for this task and has
  not been analyzed beyond this method-list transcription; recorded here
  only as a cross-reference since it shares the "acceleration structure"
  domain but not the `streamRef`-keyed request shape.
- `gputools-replay-sys::inventory::EXPORTS` notes
  `GTReplayFetchAccelerationStructure` as "the one export new in the 27 SDK
  vs 26" (see `crates/gputools-replay-sys/src/inventory.rs`, and
  `tests::accel_structure_is_noted_as_new_in_27`), which the crate's own test
  suite pins as a fact worth not losing track of: this surface did not exist
  to probe on macOS 26.

## Open questions

- What "generic" means in `GTReplayDecodeGenericAccelerationStructure`'s
  name: whether there are non-generic decode paths for specific acceleration
  structure types. No evidence either way was found; no other
  `GTReplayDecode*AccelerationStructure*` variant appears anywhere in
  `classdump-27.txt`.
- Whether `Fetch` and `Decode` requests for the same `streamRef` are both
  submitted in one batch, submitted separately, or mutually exclusive
  alternatives. This is the central open question raised by the two classes'
  identical shape (see Static findings) and is not resolvable from method
  signatures alone.
- Whether this surface's reply shares the three-key
  `unknown`/`info`/`data` shape documented for texture fetch in
  [00-texture-fetch.md](00-texture-fetch.md).
- Being new in the macOS 27 SDK, whether any existing capture in
  `captures/` (both predate this task's smoke run) even contains an
  acceleration structure to fetch; this may need a purpose-built capture
  (e.g. from a ray-tracing sample app) to probe at all.
- Whether `GTReplayUpdateAccelerationStructureSession` (see Static findings)
  is part of the same client workflow as the two fetch/decode classes, or
  fully separate infrastructure (e.g. for replaying an app's own
  acceleration-structure builds rather than fetching the replayer's copy).
  Not analyzed beyond its method list; out of this task's scope.
- No behavior has been established for `-setDispatchUID:`/`-dispatchUID` on
  either class (same gap as noted in
  [03-pipeline-fetch.md](03-pipeline-fetch.md)): whether it is required,
  caller-assigned, or has a default that must not be relied upon.

## Live probes

None run yet.

## Instance (top-level) vs primitive (bottom-level) structure (2026-09-01)

The fixture (`ACCEL_INSTANCE=1`) also builds an `MTLInstanceAccelerationStructure`
over the triangle BLAS; the capture then has two structures ("triangle_accel",
"triangle_instance"), both enumerated by `gpudebug`. Fetching both (MEASURED):

- The PRIMITIVE (bottom-level) structure fetches its full bytes (1816 B, decoded
  below), with info field `0x20` = `0x30000`.
- The INSTANCE (top-level) structure fetches a record (its own streamRef) but a
  ZERO-length payload - `size` = 0, no data - with info field `0x20` = `0x20000`.

So a top-level structure is fetchable-as-a-record but yields no content bytes on
the snapshot / `FORCE_LOAD` path, unlike a bottom-level one. The `0x20` field
(`0x30000` vs `0x20000`) is the likely structure-KIND discriminator, from one
example of each. Not established: whether tracing against the instance structure
(a real ray-intersection dispatch, which this fixture does not do) would
populate its content; and the instance-descriptor layout (transform + BLAS
index), which lives in the `instance_descriptors` buffer, itself fetchable via
`GTReplayFetchBuffer`.

## Decoded byte format (2026-09-01, by controlled variation)

The fetched acceleration-structure payload (dossier "Live findings" below) was
decoded by building the fixture with distinct vertex coordinates and diffing.
Two variants against the baseline triangle `{0,0,0, 1,0,0, 0,1,0}`:
`{0,0,0, 7,0,0, 0,11,0}` (locates X/Y-varying fields) and
`{2,3,4, 5,6,7, 8,9,10}` (fully distinct, locates every component and the bbox
min). For a single-triangle primitive (bottom-level) acceleration structure,
1816 bytes, the layout is (all little-endian; MEASURED offsets):

| offset | field | evidence |
| --- | --- | --- |
| `0x00` | `0` | constant |
| `0x04` | `2` - format/version tag | constant across variants |
| `0x08` | total size in bytes (`0x718` = 1816) | = the `size` field of the info record |
| `0x10` | `0x700` = size - 0x18 (payload after a 24-byte prefix) | constant |
| `0x18` | `0x310` - an internal section offset | constant |
| `0x20` | `1.0f` | constant |
| `0x28` | `1` - geometry-descriptor count (constant across triangle counts) | |
| `0x2c` | **triangle / primitive count** | MEASURED: 1, 2, 10 tris -> 1, 2, 10 |
| `0x34` | `1` - constant on these captures | |
| `0x0a0`..`0x0b4` | **geometry AABB**: min (x,y,z) then max (x,y,z), 6 contiguous floats | diff: min=(2,3,4), max=(8,9,10) |
| `0x228`..`0x250` | **a second AABB** (same min/max) with an 8-byte element stride - a node-bounds copy | diff: same values, stride 8 |
| `0x418`..`0x43b` | **the triangle vertices**: three tightly-packed `float3`s (12-byte stride) | diff: (2,3,4),(5,6,7),(8,9,10) - verbatim input |

So the replayer preserves the exact input geometry, and stores the AABB twice
(a compact geometry bounds at `0x0a0` and a strided node-bounds copy at
`0x228`). Size scales with triangle count above a ~1816-byte minimum (1-2 triangles =
1816 bytes; 10 triangles = 2328). The vertex section and both AABBs grow with it.

Not yet decoded: the bytes between the header and `0x0a0`, the `0x310` section
the header points at, multiple geometry descriptors (`0x28`), and an instance
(top-level) structure - all reachable by the same variation method (the fixture
takes `ACCEL_VERTS`, `triangleCount = floats/9`).

## Live findings (2026-09-01)

The first acceleration-structure capture in the campaign was built to order:
`fixture-apps/accel-structure.m` constructs a primitive `MTLAccelerationStructure`
over one triangle and builds it on the GPU (M1 Max, `supportsRaytracing`). It is
two-phase (`FIXTURE_GO_FILE`): it builds the structure, blocks until the capture
boundary is placed (`capture-late.sh`), then REFITS it inside the capture so the
structure both pre-exists the boundary and is referenced by a captured command.
`gpudebug` then enumerates `acceleration_structures: 1 object` ("triangle_accel").

`GTReplayFetchAccelerationStructure` was fetched live through the generic
`Session::fetch_raw` path. MEASURED:

- **Same reply envelope**: `bplist` `unknown`/`info`/`data`, one 80-byte `info`
  record, `data` 1816 bytes.
- **The info record uses the TEXTURE layout, not the pipeline one**: `0x00` is a
  request ordinal, **`0x08` is the streamRef** (= 4 here), `0x18` data_offset,
  `0x1c` size. So an acceleration-structure fetch is PER-RESOURCE (streamRef-
  keyed), unlike the command-stream-threshold pipeline fetch. Confirmed by
  single-ref fetches: request `[3]` returns nothing, `[4]` returns the streamRef-4
  structure.
- **The `data` payload is RAW acceleration-structure bytes, not a nested
  archive** (1816 bytes; a small header carries the size `0x718` at offset 8).
  This differs from pipeline binaries, whose payload is a nested `bplist` of
  Mach-O shader binaries.
- **Fetchable only because it is USED**: with the phase-2 refit referencing the
  structure, it fetches with no special flag. A built-but-never-referenced
  acceleration structure (single-phase capture) returned nothing even under
  `MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE=1` - corroborating the used-vs-unused
  resource rule from dossier 00.

Not yet done: decoding the raw acceleration-structure byte format, and an
instance (top-level) acceleration structure (this is a primitive/bottom-level
one).

## Status

- `GTReplayFetchAccelerationStructure` - **Behavior confirmed (2026-09-01):
  first live fetch on a purpose-built capture, reply format characterized (see
  Live findings above). Signature established instanceSize 32; `-setStreamRef:` (`Q`) +
  `-setDispatchUID:` (`(?={?=ii}Q)`) + NSSecureCoding - the same fetch-request
  base as `GTReplayFetchTexture`. The one export new in the 27 SDK vs 26.
  Pinned by `inventory::tests::the_fetch_request_classes_share_the_streamref_shape`.
  No live fetch: no capture with an acceleration structure has been identified
  (none of the three captures here has one, per `gpudebug`).
- `GTReplayDecodeGenericAccelerationStructure` - **Signature established
  (2026-09-01) from the live runtime; behavior probed and DOES NOT drive via a
  bare streamRef.** A `fetch_raw` with only `-setStreamRef:` returns a reply
  whose `unknown` key carries an `NSCocoaErrorDomain` error "unknown request"
  (info/data empty). So a "decode" differs from a "fetch": it likely needs the
  `dispatchUID` (a decode is per-dispatch) or is not served by the in-process
  replayer this way. Shape stands; the streamRef-only path is a dead end for it.
  instanceSize 32; identical
  `setStreamRef:`/`setDispatchUID:` shape. A sibling `GTReplayDecodeICB` (not in
  the tbd inventory) shares the exact shape, so "decode" requests are the same
  family as "fetch" requests at the wire level. Pinned by the same test. No
  live probe.
