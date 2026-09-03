# Surface: texture fetch

Symbols:

- `GTReplayFetchTexture` (ObjC class) - Established: fully described by the
  live runtime, instanceSize 128. Full setter list (classdump): `-setStreamRef:`
  (`Q`), `-setSize:` (`{GTSize}`), `-setRegion:` (`{GTRegion}`), `-setPlane:`
  (`I`), `-setDepth:` (`I`), **`-setSlice:` (`I`)**, **`-setLevel:` (`I`)**,
  `-setDispatchUID:` (the union), `-setResolveMultisampleTexture:` (`B`). The
  substrate/probes call only streamRef/size/region/plane/depth; `setSlice` and
  `setLevel` are the array-slice and mip-level selectors, MEASURED to work
  (2026-09-02, see "Texture format & shape coverage"). HANDOFF 2.4.
- `GTReplayUnarchiver` (ObjC class) - **Shape established (2026-09-01): the
  runtime description is complete but minimal** - instanceSize 8 with NO own
  methods, i.e. a thin `NSObject` subclass/wrapper. It exposes no distinguishing
  selectors to probe, so its role (the crate parses the `NSKeyedArchiver` reply
  by hand instead) is not established beyond the empty shell.
- `GTReplayRequestBatch` (ObjC class, runtime-only: not in the tbd, registered
  when the framework loads) - Established as far as its two setters used in
  the bootstrap path: `-setRequests:` (NSArray), `-setCompletionHandler:`
  (block). HANDOFF 2.4.
- `GTMTLReplayService` (ObjC class) - Established: `-initWithContext:`,
  `-load:error:` (takes `NSURL`, not a path string or a `GTReplayLoadRequest`),
  `-fetch:`. HANDOFF 2.2/2.4. Listed here because `-fetch:` is the entry point
  that dispatches a `GTReplayFetchTexture` batch, even though the class itself
  is shared infrastructure.

## The fetch family (generalised, 2026-09-01)

`Session::fetch_raw` (probe `rawfetch`) drives ANY `GTReplayFetch*`/`GTReplayDecode*`
request class through the established `GTReplayRequestBatch` + `-fetch:` path,
setting only `-setStreamRef:`. Sweeping the classes across the three real
captures plus the purpose-built fixtures establishes a common contract and two
axes of variation. MEASURED:

**The shared envelope.** Every reply is an `NSKeyedArchiver` `bplist` whose
payload dict has the three keys `unknown` / `info` / `data`. `info` is a table
of 80-byte records; `data` is the concatenated payloads; the record's
`data_offset` (`0x18`) and `size` (`0x1c`) accumulate. `reply::parse_reply`
decodes all of them unchanged.

**Axis 1 - keying.** Most fetches are PER-RESOURCE: the info record uses the
texture layout with the streamRef at `0x08`, a single-ref request answers only
that ref, and a ref with no resource answers nothing. Pipeline binaries are the
exception: the requested streamRef is a command-stream THRESHOLD and the reply
is the cumulative set up to that point, with a sequential id at `0x00` and a
handle at `0x08` (dossier 03).

**Axis 2 - payload.** Per-resource fetches return the resource's RAW bytes
(texture pixels; buffer/heap/acceleration-structure bytes). Pipeline binaries
return a NESTED `bplist` per record (compiled Mach-O shader binaries +
statistics, dossier 03).

| class | in tbd? | keying | payload | evidence |
| --- | --- | --- | --- | --- |
| `GTReplayFetchTexture` | yes | per-resource | raw pixels | ground truth + gpudebug oracle |
| `GTReplayFetchBuffer` | no (runtime-only) | per-resource | raw bytes | GROUND TRUTH: fixture buffer ref 2 = the exact triangle `{0,0,0,1,0,0,0,1,0}`; 580 buffers on corpus |
| `GTReplayFetchHeap` | no (runtime-only) | per-resource | raw bytes | 410 heaps on corpus, 164 MB |
| `GTReplayFetchAccelerationStructure` | yes | per-resource | raw bytes | fixture capture (dossier 05) |
| `GTReplayFetchPipelineBinaries` | yes | command threshold | nested Mach-O archive | corpus, 13+ pipelines (dossier 03) |
| `GTReplayDecodeGenericAccelerationStructure` | yes | - | - | replies "unknown request" to a bare-streamRef fetch (dossier 05) |
| `GTReplayFetchWireframe` | no (runtime-only) | dispatch-keyed (draw index) | rendered wireframe image | 62/98 draws on corpus (item 11) |

**Two request families.** Not every fetch class is streamRef-keyed.

DISPATCH-KEYED FETCHES ARE NOW DRIVEN LIVE (2026-09-01, item 11). The
`dispatchUID` encoding `(?={?=ii}Q)` is an 8-byte UNION - two `int32`s or one
`uint64` - and in practice a small integer DRAW INDEX. `GTReplayFetchWireframe`
(`-setDispatchUID:` + `-setSolid:`), swept over dispatchUID `0..500` on
`corpus.gputrace`, answered for 98 draws (dispatchUIDs 325..422).
MEASURED:

- Same reply envelope (`bplist` unknown/info/data, 80-byte info records) and the
  TEXTURE record layout, but keyed differently: `0x08` (streamRef) is `-1`,
  `0x10` echoes the dispatchUID, and `0x30`/`0x34`/`0x38` are width/height/format
  as usual.
- The payload is a RENDERED wireframe IMAGE per draw (R8, e.g. 320x288, ~1% of
  pixels non-zero = sparse wireframe lines). 62 of the 98 draws have non-blank
  wireframes; render-target sizes vary (320x288, 320x504, 320x1008).

So a dispatch-keyed fetch renders debug imagery FOR a specific draw, identified
by a small-integer dispatchUID, and returns it in the same envelope. The other
dispatch-keyed classes follow the same shape with their extra setters
(`GTReplayFetchThreadgroup`/`ImageBlock` add `-setIndex:`, `GTReplayFetchPostVertex`
adds object-shader threadgroup bounds). The type is in `Session::DispatchUid`
and the driver in `Session::fetch_wireframe`.

The original two-family split, for reference. Not every fetch class is streamRef-keyed. Reading
their live shapes splits the family in two:

- RESOURCE-KEYED (`-setStreamRef:`, optional `-setDispatchUID:`): texture,
  buffer, heap, acceleration structure, pipeline binaries. Fetch a resource by
  its streamRef; driven by `fetch_raw`.
- DISPATCH-KEYED (`-setDispatchUID:` + per-class params, NO `-setStreamRef:`):
  fetch data produced BY a specific draw/dispatch. `GTReplayFetchThreadgroup`
  and `GTReplayFetchImageBlock` take `-setIndex:` + `-setDispatchUID:`;
  `GTReplayFetchWireframe` takes `-setSolid:` + `-setDispatchUID:`;
  `GTReplayFetchPostVertex` takes `-setDispatchUID:` + object-shader threadgroup
  bounds. These need a valid `dispatchUID` from the command stream, so
  `fetch_raw` (streamRef-only) cannot drive them and now fails cleanly with
  "class is not streamRef-keyed" rather than a missing-selector panic. The
  `GTReplayDecode*` classes take `-setStreamRef:` + `-setDispatchUID:` but reply
  "unknown request" to a streamRef-only fetch (a decode is per-dispatch too).

So the safe crate's fetch API is one mechanism (batch of request objects,
shared reply envelope) with a per-class record decoder and a per-class payload
interpretation. `GTReplayFetchBuffer`/`Heap` are real, working fetch classes not
in the 31-symbol tbd inventory (runtime-only, like `GTReplayRequestBatch`).

## The bundle index bridges to the fetch streamRef by ORDERING (2026-09-02, MEASURED)

**Question.** A `.gputrace` bundle's `index` file enumerates its resources, and
a sibling project (`gpu-trace-parse-rs`, `gputrace/src/index/`) decoded that
format: a 20-byte `xdic` header (bucket count, record count), a 12-byte-bucket
open-addressed hash table `(canonical_record, this_record, 0xFFFFFFFF)` from
offset 20 (an earlier parse here misread it as 8-byte slots), a 24-byte record
array (each record carries a `store0` offset - where its data lives in the
store), a tail name table, and 248-byte **texture descriptors** decoding pixel
format, width, height, depth, mip count, array length, texture type, usage, and
a per-texture `textureId`. The descriptor count matches `gpudebug`'s texture
count exactly (180 on corpus). The question was whether the bundle maps to
the **fetch streamRef** (`gpudebug`'s `resourceIndex`, the value our fetch keys
on), so a fetched texture could be tagged with its mip/array/type.

**No descriptor FIELD is the streamRef** (confirmed, reproducing
`gpu-trace-parse-rs`). Swept `corpus.gputrace` through
`gputools-replay-hl` (`textures(0..=3000)`) -> 180 answering streamRefs with
width/height/`MTLPixelFormat`; read all 180 descriptors (`gputrace info --json`)
plus each record's `store0` offset (`gputrace objects`). No descriptor column
(`id`, `textureId`, any byte offset) equals the streamRef: the 10 `RGBA32Float`
textures fetch at refs `1095..1113` while their descriptor `id`s are `36..722`
and `textureId`s `59..81` - disjoint. `gpu-trace-parse-rs` reports the same
field-negative from its side (only the non-injective `allocationID` joins).

**But the streamRef IS the store0-offset RANK** (MEASURED - the ordering a
field-sweep cannot see). Sort the bundle's texture descriptors by `store0`
offset ascending; sort the answering streamRefs ascending; they correspond
1:1, rank for rank. Evidence, strongest first: the uniquely-dimensioned
textures (pinnable by dims+format alone) have streamRef-rank and offset-rank
in perfect agreement - Spearman rho = 1.0 (corpus: 9 such anchors, chance
~1/9! ~ 3e-6; `known-textures-late`: 7/7). Full-sequence: the run-length
encodings of the two dims sequences are identical (corpus 26/26 runs).

**Validated across every joinable bundle** (2026-09-02): corpus (180),
known-textures-late (7), known-stencil (6), known-depth / known-depth-stencil
(3), known-3d / known-mips / known-astc / known-ycbcr (2), known-draws (1) -
208 texture matches, ZERO ordering violations. In each, fetch-refs-ascending
zip descriptors-by-offset-ascending with dims equal at every rank and both
orders strictly increasing. Descriptor `id`s are NOT in ref order (e.g.
known-3d: refs 2,3 <-> ids 9,6 but offsets 172,244) - the `store0` OFFSET is,
which is exactly why the field-sweep missed it. `small.gputrace` and the
`accel-*` bundles are NOT validated: `gpu-trace-parse-rs` yields 0 texture
descriptors for their indices.

**Consequence for the safe crate.** This is a **post-sweep join**, not static
enumeration: the streamRef *values* are assigned by the load path and are not
computable from the bundle (the bundle gives rank -> descriptor, not
value -> descriptor), so a fetch sweep is still required to learn them. After
sweeping, `sorted(refs)[k]` <-> `sorted(descriptors_by_offset)[k]`, so each
fetched texture can be tagged with its descriptor's mip count, array length,
texture type, and usage. One caveat a fidelity-first API must respect:
- **The bundle is a SUBSET.** The fetch serves textures absent from the
  descriptor list - corpus's 2 runtime `960x864` framebuffers,
  known-stencil's separately-fetchable depth aspect (fmt 252). The join walks
  streamRefs ascending against descriptors-by-offset ascending, leaving an
  unmatched ref as a descriptor-less extra, and hard-errors if a descriptor
  cannot be placed - it never force-zips an extra into a descriptor slot.

**Intra-dims-group order is MEASURED, not inferred** (2026-09-02,
`known-ambiguous.gputrace`). Earlier this was the open caveat: within a run of
identical-dims textures the rank-zip assigned an order the fetch could not
verify. The `known-ambiguous` fixture resolves it - three 64x64 BGRA textures
with distinct mip counts (1/3/7) AND distinct clear colours (red/green/blue), so
the fetched pixels identify each physical texture and construction pins
colour->mip. MEASURED: streamRefs 2,3,4 (= red,green,blue) map rank-for-rank to
descriptors sorted by store0 offset 505,575,666 (= mip 1,3,7). So
streamRef-rank == store0-offset-rank holds WITHIN a run, and the join attributes
inside same-dims runs deterministically, not heuristically.

**The fetch reply carries NO descriptor key** (2026-09-02, MEASURED, the reason
the join is ordinal rather than a key lookup). The 80-byte texture reply record
was fully mapped on `known-ambiguous`: across the three textures every field is
identical except `streamRef` and geometry. The unmapped bytes are constant
(`0x20 == 0x2c == 65536`, the rest zero) - no `texture_id` (descriptor word 17),
no `allocationID`, no manifest pointer. The descriptor holds no `streamRef`
either. So the link between a fetched texture and its descriptor is purely
ordinal (both sides in resource-creation order); there is no stored key to join
on, and `texture_id` is NOT usable as an order key (MEASURED non-monotonic with
store0 offset on corpus: 160,162..175,67,66,84,..).

**Combined depth-stencil is 1:1 with ONE streamRef; the fetch exposes ASPECTS,
the descriptor is the RESOURCE** (2026-09-02, MEASURED on `known-ds-pair`,
content re-verified 2026-09-03 after a fixture fix). A combined
`Depth32Float_Stencil8` resource is ONE manifest descriptor (raw format 260) and
ONE fetched streamRef; **plane 0 serves the depth aspect (format 252), plane 1
the stencil aspect (format 261), on the SAME streamRef**. Content is byte-exact
per aspect (the blit-stored `ds_dst` targets): the 64x64 resource's depth aspect
reads `00 00 80 3e` (float 0.25) and its stencil aspect `0b` (11); the 96x96
resource's depth `00 00 40 3f` (0.75) and stencil `16` (22) - exactly as
constructed, from separate plane-0/plane-1 fetches of one streamRef. (The
fixture originally re-allocated its textures every phase, so it stored no
content and only the 64x64 pair answered; fixed to allocate once + re-render, it
now stores content and BOTH dims answer - 6 descriptors, 6 fetched. The 1:1
structural claim held regardless, as it reads from the record dims/formats, and
`known-depth-stencil` independently stores 0.5/42.) So a plain
`textures()` (plane-0) sweep returns ONE texture per combined resource - the
depth aspect (252) - not two, and not the combined format. It is 1:1 by
resource; only the FORMAT diverges (descriptor 260 vs fetched aspect 252/261),
never the count. This is the aspect-vs-resource gap: the fetch is
sub-resource-addressed (the replayer can only serve aspects - a combined
depth-stencil cannot be dumped as one blob), while the manifest is
resource-addressed. A base `Depth32Float` (252) and the depth aspect of a
combined resource BOTH fetch as 252/plane-0, so they are indistinguishable from
the fetch alone - only the descriptor (252 vs 260) tells them apart.

Corpus raw descriptor formats (via the `gputrace-bundle` parser) confirm
the ONLY divergent multi-plane format is combined depth-stencil (260): base
depth (252), base stencil (253), YCbCr (stored as SEPARATE R8/RG8 = 10/30
resources, each format-matching its fetch), and ASTC (204) are all single-plane
and match the fetched format exactly. So the descriptor join matches on exact
`(w, h, format)`: single-plane resources attribute correctly (format-exactness
prevents any cross-format mixup within a same-dims run), and combined
depth-stencil descriptors are transparent to the walk (skipped, not required to
place; their aspect fetches carry no descriptor). Reassembling a combined
resource into a resource-level view with named raw aspect planes is deferred
(it would need both-plane fetches and generalizes to a resource-vs-sub-fetch
model; no color-only target needs it).

Notes: `gpu-trace-parse-rs` names depth/stencil/ASTC/YCbCr descriptor formats
"unknown" (it does not decode those enums) but their dims+offset still align;
corpus's sweep returns refs 1121 and 1122 twice (real, reproduced on a
clean sweep - the fetch emits duplicate records for those two, deduped before
joining). **3D detection is NOT available from the fetch** - it requires the
bundle descriptor's `texture_type` (via `describe()`). The fetched record's
`depth` field (`0x36`) is NOT a 3D signal: the replayer serves one fixed z-plane
of a `Type3D` texture and reports THAT PLANE's depth, so `depth` reads 1 for a
volume (MEASURED, `live_hl_provenance_3d.rs`: `Texture::depth()` == 1, not 4, for
the 16x16x4 `known-3d` volume; `Texture::depth()`'s doc says the same). The only
3D signal is the manifest descriptor's `texture_type == Type3D`.


## Out-of-range level/slice: the fetch CLAMPS, never errors (2026-09-03, MEASURED)

Probed `known-mips.gputrace` (2D-array, 2 slices, 7 mip levels 64->1, BGRA) with
`GTReplayFetchTexture` at valid and out-of-range `level`/`slice`. The fetch does
NOT error, and does NOT return nothing, for an out-of-range request - it returns
plausible-looking data, so **blind "probe until it stops answering" is unsafe;
iterate the descriptor's `mip_levels`/`array_length` instead** (both exposed by
the bundle-descriptor join).

- **Level.** 0..6 halve correctly (`max(1, 64>>level)`: 64,32,16,8,4,2,1). A
  request for level 7 or 8 (past the 7-level chain) **clamps to the last real
  level** (level 6, 1x1, size 4) - duplicate smallest-mip bytes, NOT level 0 and
  NOT an error. A per-level dims check (`max(1, base>>level)`) catches an
  over-long count EXCEPT at the 1x1 tail of a full mip chain, where every
  spurious level's expected dims are also 1x1 and so the check passes on
  duplicate bytes. For a chain that does not reach 1x1, the clamp returns the
  last level's larger dims and the dims check DOES catch it.
- **Slice.** slice 0 = red, slice 1 = green (valid). slice 2 and slice 3 (past
  the 2-slice array) each return a full 64x64 texture whose content is NEITHER
  valid slice (observed BGRA `ff 00 ff ff`), and slice 2 == slice 3. A dims
  check does NOT catch this (the dims look valid), so an over-long
  `array_length` guess would emit plausible-looking WRONG data.

**Record fields `0x48`/`0x4c` self-describe the requested plane/slice/level**
(2026-09-03, MEASURED across plane 0/1 x slice 0/1 x level 0/2/3). The 80-byte
texture reply record echoes the request's addressing verbatim, decodable as:
`slice = 0x48 >> 16`; `plane = (0x4c >> 8) & 0xff`; `level = 0x4c & 0xff`
(e.g. plane1/slice1/level2 -> `0x48 == 0x10000`, `0x4c == 0x102`). This is the
reliable provenance source for `Texture::plane()/slice()/level()` - each record
carries its own addressing, so no reply->request matching is needed (the
`request_ordinal` at `0x00` is contiguous WITHIN a fetch batch but its absolute
base drifts between batches, so it is NOT a usable cross-call key). The substrate
now decodes these into `TextureRecord.plane/slice/level` (previously in the
`unmapped` map).

**No in-band end signal** (2026-09-03, thorough re-vet: all record fields + full
payloads, levels 5/6/7/8/20 and slices 0/1/2/3/10). The `0x4c`/`0x48` fields
above echo the REQUEST, not validity, so an out-of-range value echoes itself
(`0x4c` = requested level: L7->7, L20->20; `0x48` = requested slice `<< 16`:
S2->0x20000, S10->0xA0000). They parrot
the request, so an out-of-range value echoes itself and signals nothing. Out-of-
range LEVELS return byte-identical copies of the last valid level (L6/L7/L8/L20
all `00 00 ff ff`); out-of-range SLICES return a constant out-of-bounds read
(S2/S3/S10 all `ff 00 ff ff`, neither valid slice). Every out-of-range request
still returns a record (never an empty reply). So nothing in the reply
distinguishes a valid tail level/slice from an out-of-range one.

Consequence: the descriptor's `mip_levels` and `array_length` are the only
reliable iteration bounds; a probe loop CANNOT self-terminate safely. Absent a
descriptor (unparseable index, or `small.gputrace`), the only in-band cues are
heuristic and weak: for a full mip chain, dims reaching 1x1 marks the end; two
consecutive levels/slices returning byte-identical payloads suggests running off
the end (but genuinely-identical content false-positives it). Neither is
definitive.

## Texture format & shape coverage (2026-09-02, gputools-replay-hl spikes)

Purpose-built fixtures (`fixture-apps/known-{depth,depth-stencil,stencil,ycbcr,
mips,3d,astc}.m`) established how `GTReplayFetchTexture` serves every texture
format and shape. All MEASURED against ground truth; the record's format lives
at `0x38` (a `MTLPixelFormat` `u32`). Key result: **the replayer serves every
non-color texture as an ordinary single-aspect 2D image in its native format**,
so the whole space reduces to the generic record + payload path.

- **Depth.** `Depth32Float` (fmt 252) fetches as native f32, one per pixel,
  standard `bytes_per_row`; ground truth 0.5 everywhere (`known-depth`).
- **Combined depth+stencil.** A `Depth32Float_Stencil8` texture NEVER returns as
  the combined fmt (260) - no 8-byte packed record exists. Both aspects are
  instead selectable FROM THE SAME streamRef via the fetch `-setPlane:`
  parameter (CONCLUSIVE, 2026-09-02, `known-depth-stencil` ref 4 with a stored
  depth 0.5 + stencil 42):
  - `plane 0` -> the DEPTH aspect as `Depth32Float` (fmt 252, 4 B/px, reads 0.5).
  - `plane 1` -> the STENCIL aspect as `X32_Stencil8` (fmt 261, 1 B/px, reads 42).
    `plane >= 1` clamps to the stencil aspect. (fmt 261 = `X32_Stencil8`, the
    stencil view of `Depth32Float_Stencil8`; 262 = `X24_Stencil8`.)

  So `plane` is the aspect selector for a combined texture; you get everything
  (depth AND stencil) from one streamRef in two fetches. An app-created
  `X24/X32_Stencil8` texture VIEW (`known-stencil`) surfaces the stencil aspect
  as its OWN separate resource too, but that view is NOT required - `setPlane`
  is the direct lever. (This corrects the earlier reading that the stencil
  aspect was reachable only via a view.)
- **Stencil.** Base `Stencil8` (fmt 253) fetches as 1 B/px; ground truth 42
  (`known-stencil`). Stencil reads as `u8` in every case. (Content storage for a
  stencil-view aspect is capture-dependent - a fetched view may be a placeholder
  if the underlying combined texture was not stored as a blit source.)
- **Planar / YCbCr.** A biplanar 4:2:0 `CVPixelBuffer` fetches as TWO separate
  ordinary texture records - luma `R8Unorm` (full res, 128) and chroma
  `RG8Unorm` (half res, Cb 100/Cr 150) - each its own streamRef, byte-exact. No
  plane parameter needed; planar = per-plane R8/RG8 textures (`known-ycbcr`).
- **Array slices + mip levels.** Addressable by index via `-setSlice:` /
  `-setLevel:` (u32), which the substrate/probes never called. Wiring `setSlice`
  and fetching a 2D-array returns slice 0 = red, slice 1 = green exactly
  (`known-mips`). So each slice/level is its own fetchable 2D image; the default
  (no setter) returns slice 0 / level 0.
- **3D volumes.** The one gap: NO parameter (`setSlice`, `setLevel`,
  `region.origin.z`, `plane`) selects a z-plane of a `Type3D` texture - every
  combination returns one fixed plane. `GTReplayFetchTexture` has no
  `-setDepthPlane:` (the `slice` vs `depthPlane` distinction lives on
  `GTCaptureArchiveHeapRestoreTextureSliceOverrideKey`, the capture-archive
  restore path, not the fetch request). 3D z-plane addressing is unexposed
  (`known-3d`). Because the fetch serves one fixed plane, the record's `depth`
  field (`0x36`) reports THAT plane's depth (1), not the volume's - so `depth` is
  NOT a 3D signal (MEASURED `live_hl_provenance_3d.rs`); detect 3D from the
  manifest descriptor's `texture_type` via `describe()`.
- **Compressed (ASTC).** `ASTC_4x4_LDR` (fmt 204) fetches as its RAW COMPRESSED
  blocks byte-for-byte (256 blocks x 16 B = 4096 B, `bytes_per_row` = blocks/row
  x 16); the replayer does NOT decompress (`known-astc`).

Storage caveat carried from `known-textures`: a render/compute target that is
only WRITTEN is not snapshotted for fetch; making it a blit SOURCE stores its
content. All the shape fixtures blit-store their target for this reason.

## Static findings

- **Fetch object shape** (MEASURED, read off the live Objective-C runtime via
  `class_getInstanceMethod` / `method_getTypeEncoding` on
  `GTReplayFetchTexture`, HANDOFF 2.4): instanceSize 128, with settable
  properties `streamRef` (u64), `size` (`GTSize{u64 w,h,d}`), `region`
  (`GTRegion{GTPoint3D{u64 x,y,z}, GTSize}`), `plane` (u32), `depth` (u32),
  plus `resolveMultisampleTexture` and `dispatchUID` fields whose setters are
  not yet exercised by this crate.
- **`depth` must be 1** (MEASURED against a real capture, prior project;
  HANDOFF 2.4): a batch with `depth = 0` on every request returns nothing. A
  texture always has at least one slice; a zero was misread as "the empty
  region convention" on the prior project and cost weeks. Preserve `depth = 1`
  as a hard invariant in the request builder, not a default that can drift.
- **A zero-sized region returns the texture at its natural size, unresampled**
  (MEASURED, HANDOFF 2.4). This is the "natural size" fetch mode this crate's
  `smoke` probe exercises.
- **A non-zero region resamples, it does not crop** (MEASURED, HANDOFF 2.4):
  the framework scales to fit, preserving aspect ratio, in both directions.
  There is no request size that "caps out" at the natural size; resampling
  always happens once the region is non-zero.
- **`streamRef`s are sparse** (MEASURED, HANDOFF 2.4): a 0..2000 sweep on
  `corpus.gputrace` returned 182 records, not a contiguous range.
  Callers must sweep and keep whatever answers, never assume a dense id space.
- **Never `waitUntilCompleted`** (established via the prior project's working
  implementation, HANDOFF 2.4): the completion block for `-fetch:` arrives on
  a thread this crate does not own; the caller must pump the run loop instead
  of blocking on it.
- **Reply format** (MEASURED against real replies, HANDOFF 2.5): the response
  is an `NSKeyedArchiver` binary plist with exactly three top-level keys:
  - `unknown` - an `NSArray`. Empty in every reply measured, including a sweep
    where 1,818 of 2,000 requested refs went unanswered. It is **not** a list
    of unresolved requests; an earlier guess to that effect on the prior
    project is known wrong and must not be repeated.
  - `info` - a descriptor table, **80 bytes per texture record**.
  - `data` - concatenated raw pixel bytes for every answered texture.
- **Mapped `info` offsets** (MEASURED by cross-referencing known field values
  against byte offsets across real replies, HANDOFF 2.5). 11 further offsets
  in the 80-byte record are unmapped; in every record observed they hold
  either zero or the constant `0x10000`.

  | offset | field |
  | --- | --- |
  | 0x00 | request ordinal (u32) - NOT a streamRef, see the correction below |
  | 0x08 | **streamRef** (u32) |
  | 0x18 | payload offset into `data` (u32) |
  | 0x1c | payload size (u32) |
  | 0x30 | width (u32) |
  | 0x34 | height (u16) |
  | 0x36 | depth (u16) |
  | 0x38 | MTLPixelFormat (u32) |
  | 0x40 | bytesPerRow (u32) |
  | 0x44 | bytesPerImage (u32) |

- **The fetch path matches Apple's own tool on a real texture, pixel for
  pixel modulo colour management** (MEASURED 2026-09-01, `probes/run.sh texbmp`
  + `gpudebug` as oracle). Our fetch of `small.gputrace` streamRef 24
  (`gpudebug`'s `tex0`, resourceIndex 0x18) and `gpudebug`'s own PNG export of
  the same texture are the same 2880x2592 image. The per-pixel difference is a
  single monotonic per-channel tone curve - `gpudebug` value best-fits
  `ours ** 0.84` (mean abs residual 2.6/255, ~1%, down from 9.7 raw), which is
  exactly the ~2.2 -> 1.8 gamma of the "Generic RGB Profile" `gpudebug` tags
  its PNG with (confirmed via `sips -g profile`). So our raw bytes are the
  faithful, untransformed texture; `gpudebug` re-encodes them for a legacy
  display profile. Fetching the same texture before and after `play_all()`
  matches `gpudebug` equally, corroborating that playback preserves used
  content (dossier 01). This is validation against a second, independent
  implementation on a real 7.5-megapixel texture, not just against ground truth
  on a synthetic fixture.
- **The fetch + decode path is byte-correct against ground truth**
  (MEASURED 2026-09-01, `probes/run.sh pixelcheck`). `fixture-apps/known-textures.m`
  fills each texture with one exact solid colour. The textures the capture
  keeps a snapshot of (the blit source and destination) fetch and decode to
  EXACTLY that colour - every pixel, zero mismatches - so the streamRef ->
  reply -> record -> payload chain and the offset table above are correct on
  known data, not merely self-consistent. (This same probe also shows in-process
  PLAYBACK does not reproduce correct contents; that is a replay-engine result,
  see [01-playback.md](01-playback.md) "Replay correctness".)
- **The coverage gap is NOT a defect in this crate: Apple's own tool fails
  identically** (MEASURED 2026-09-01, `gpudebug(1)` as an oracle).

  A purpose-built fixture (`fixture-apps/known-textures.m`) captured with
  `gpucapture` creates six textures with exact ground truth, varying one
  property per row: storage mode (Private/Shared), usage (RenderTarget with
  and without ShaderRead), whether anything reads it (blit source), and how it
  is written (render-pass Clear / blit destination / CPU `-replaceRegion:`).

  - Our fetch path answers **0 records** for that capture, sweeping streamRefs
    `0..=20000`, both before and after `play_all()` (the trace replays fine -
    95 commands, index advances).
  - `gpudebug` ENUMERATES all six correctly, with our labels, dimensions,
    formats, storage modes and usages, and reports each as fetchable. Their
    `resourceIndex` values are 0x3..0x8, well inside the swept range.
  - `gpudebug fetch` on each of the six fails: `error: empty or invalid
    response data`. **All six. Every property variant.**
  - Control, same tool, same session shape: `gpudebug fetch tex0` on
    `small.gputrace` succeeds, writing a 928 KiB PNG - and our path answers 4
    records on that capture. So the oracle works; the fixture capture is
    genuinely unfetchable.

  Both implementations agree on both captures. Whatever the gap is, this crate
  reproduces Apple's behaviour rather than falling short of it - which also
  means no amount of work on the request-encoding side will close it.

  Consistent with this, `gpudebug` shows `small.gputrace` as 7 textures of
  which exactly the 4 that answer carry dimensions in its listing; the 3 that
  never answer show no dimensions there either. The replayer appears to have
  no contents for them, not to be refusing to hand them over.

  SUPERSEDED the same day by the "Coverage: MEASURED and largely explained"
  fact below: the discriminator is whether the captured commands USE the
  texture, and `MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE=1` makes every one of
  these six fetchable. Ruled OUT along the way: storage mode, usage flags,
  being read by a later command, how the contents were written, and (by the
  two-phase capture) whether the texture pre-existed the capture boundary.

  A discarded lead worth not repeating: the two answering captures both contain a
  `*.gputrace.gpuprofiler_raw` and the fixture does not, but that file is
  optional Xcode profiling data added on export, not replay input - the
  correlation is an artifact of both being Xcode exports.
- **CORRECTION (2026-09-01): the first two record fields were swapped.**
  0x00 is not the streamRef and 0x08 is not a separate "resourceIndex" - 0x08
  IS the streamRef, and 0x00 is a per-session request ordinal.

  MEASURED with single-ref fetches (`probes/run.sh refmatch`,
  `small.gputrace`), which a full sweep can never distinguish because the two
  fields move together in a fresh sweep:

  | request | records | 0x08 | 0x00 |
  | --- | --- | --- | --- |
  | `[24]` | 1 | 24 | 1 |
  | `[26]` | 1 | 26 | 3 |
  | `[27]` | 1 | 27 | 5 |
  | `[99]` | 0 | - | - |
  | `[25,26,27,28]` | 3 | 25,26,27 | 9,10,11 |
  | `[0..=2000]` | 4 | 24,25,26,27 | 43,44,45,46 |

  0x08 echoes the requested ref exactly; 0x00 keeps climbing across the
  session (the same four resources answer 2060..2063 and then 4062..4065 on
  later fetches in one process). On the FIRST sweep of a fresh session a ref's
  first answer is ordinaled at its 1-based request position - ref 24 is the
  25th request of a `0..=2000` sweep, hence 25 - which is exactly why 0x00
  read as a streamRef with a `+1` quirk. `reply.rs` now names these fields
  `request_ordinal` and `stream_ref`, and
  `the_request_ordinal_is_not_the_stream_ref` pins the distinction.

  What is NOT established: what 0x00 actually counts. It advances by more than
  the number of requests submitted, so something else increments it too. Never
  key on it and never compare it across fetches.

  This may also explain the plane-1 anomaly below - reading the ordinal as a
  streamRef would make the "returned streamRef" mismatch the request. That is
  a LEAD, not a finding: nobody has re-run the plane-1 case since.
- **One streamRef can answer more than once** (MEASURED, HANDOFF 2.5, re-read
  under the corrected field names): a real 182-record reply had only 180
  distinct streamRefs. The two repeats carry the same size, format and
  dimensions but different payload offsets and much higher ordinals (ref 1121
  answers at ordinal 1122 and again at 1554). The identity of a fetched record
  is the pair `(streamRef, plane)`. The prior tool shipped a silent-overwrite
  bug from keying filenames on a non-unique field; the `reply` module test
  `one_stream_ref_can_answer_more_than_once` guards it going forward.
- **The reply's streamRef matches the request, at plane 0** (MEASURED, and now
  confirmed directly by the single-ref table above rather than inferred from a
  sweep). At plane 1 the returned value did not match the request (see Open
  questions), so this fact's scope is plane 0 only.
- **Observed pixel formats** (MEASURED across the two committed captures,
  HANDOFF 2.6 and `captures/README.md`): `MTLPixelFormat` 10 (R8Unorm), 70
  (RGBA8Unorm), 80 (BGRA8Unorm), 125 (RGBA32Float). No multi-planar format has
  ever been observed in any capture available to this campaign.
- **Class-dump cross-reference**: `GTReplayFetchTexture` and
  `GTReplayUnarchiver` both appear in the macOS 27 live-runtime class dump
  (`docs/findings/raw/classdump-27.txt`, Task 8), confirming both are real
  classes registered by the framework, independent of the tbd export list.

## Open questions

- **`plane:` - RESOLVED (2026-09-02): it selects the ASPECT of a combined
  depth/stencil texture.** On a `Depth32Float_Stencil8` texture, `plane 0` returns
  the depth aspect (fmt 252) and `plane 1` returns the stencil aspect (fmt 261),
  from the SAME streamRef, CONCLUSIVELY (`known-depth-stencil` ref 4: plane 0 =
  depth 0.5, plane 1 = stencil 42). For biplanar YCbCr (`known-ycbcr`), `plane:`
  is NOT the mechanism - a CVPixelBuffer's luma/chroma planes are captured as two
  SEPARATE resources (own streamRefs, `R8Unorm` + `RG8Unorm`), each fetched with
  `plane: 0`. And on a plain single-plane or 3D texture, varying `plane:` does
  nothing (measured). So `plane:` = "which aspect of a combined depth/stencil
  texture"; it is inert for ordinary and separately-resourced (YCbCr) textures.
  See "Texture format & shape coverage".
- **`slice:` - RESOLVED, `level:` - mechanism confirmed (2026-09-02).**
  `-setSlice:` selects array slices: a 2D-array fixture returns slice 0 = red,
  slice 1 = green exactly (`known-mips`). `-setLevel:` is present on
  `GTReplayFetchTexture` (same `u32` setter family) as the mip-level selector;
  its per-level behaviour was not varied-tested but the mechanism is confirmed.
  Both were simply never called by the substrate/probes. See "Texture format &
  shape coverage".
- **Coverage: MEASURED and largely explained (2026-09-01).** The "gap" was
  mostly a category error, and the remainder splits into two understood
  kinds.

  1. **The streamRef space covers every captured object, not just textures.**
  `gpudebug` lists `corpus.gputrace` as buffers 580, heaps 410,
  fences 256, render_pipelines 35, libraries 28, samplers 24, ... and
  **textures 180**. Our `0..=2000` texture sweep answers **180**. Texture
  coverage there is complete; the other ~1,340 silent refs are objects a
  `GTReplayFetchTexture` request cannot answer by definition. `gpudebug`'s
  `resourceIndex` is the very value our fetch keys on (its 0x18..0x1b are our
  24..27 on `small.gputrace`), which independently confirms the 0x08 field
  correction above.

  2. **On `small.gputrace`, 3 of 7 textures are silent, and they are the
  transient ones.** With `info --all` on each: the four that answer (24..27)
  are Private, ShaderRead|ShaderWrite, created before the frame; the three
  that never do are the `CAMetalLayer Display Drawable` (108, Shared,
  RenderTarget), a texture VIEW created mid-frame (109, parent tex0), and a
  second drawable with **0 bytes allocated** (111). Not view-ness as such: 26
  is also a view (of tex1) and answers. Creation-during-the-frame plus
  drawable/transient-ness is the pattern; a definitive rule is NOT
  established.

  3. **Resources the captured commands never touch are "unused", not loaded
  by default, and unfetchable - and config bit 10 fixes that.** Established
  with the fixture (`fixture-apps/known-textures.m`, two-phase mode via
  `capture-late.sh`, so one trace holds six textures that pre-existed the
  boundary plus one created inside it):

  | replayer config | load | refs answering (width) |
  | --- | --- | --- |
  | default | FAILS: Code=150 "Metal object creation failed", `GTErrorKeyResourceUnused=true`, stream 6 | - |
  | `MTLREPLAYER_IGNORE_UNUSED_RESOURCE=1` | ok | 3: 64, 80, 112 (blit src, blit dst, late-created) |
  | `MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE=1` | ok | **7: every texture in the fixture** |
  | BOTH `IGNORE`=1 and `FORCE_LOAD`=1 | ok | 3: 64, 80, 112 (same as IGNORE alone) |

  **IGNORE overrides FORCE_LOAD** (2026-09-03, MEASURED with both env vars set,
  `known-textures-late`): setting both answers 3, the same as IGNORE alone, not 7
  - the replayer honours ignore-unused over force-load, so the two are NOT
  additive and `ignore_unused_resources` silently defeats
  `force_load_unused_resources`. Set at most one.

  The three that answer under IGNORE are exactly the three the captured
  commands use, and exactly the three with `MTLTexture-N-...` content files
  in the bundle (32 KiB, 32 KiB, 64 KiB); the clear-only textures get no
  content file. Pre-existing vs created-inside-the-capture makes no
  difference (64 and 80 pre-existed, 112 did not). On the two real captures
  `FORCE_LOAD_UNUSED_RESOURCE=1` changes nothing (4 -> 4, 180 -> 180): real
  frames use their textures, so the lever only matters for synthetic captures
  like this fixture, and it does NOT reach `sample`'s three transient ones.

  4. **Load-error policy differs from Apple's tool, but tolerance alone does
  not help.** `gpudebug` reaches "Replayer ready" on the fixture trace where
  our `-load:error:` returns the Code=150 error and we abort. With a
  probe-only `PROBE_TOLERATE_LOAD_ERRORS=1`, load proceeds and the SAME error
  resurfaces on the first fetch (our observer attributes it); after
  `FORCE_LOAD` it resurfaces during `play_all()` instead. So the replayer
  genuinely cannot create trace stream 6 on this capture whenever asked, and
  `gpudebug fetch`'s "empty or invalid response data" on the same trace is
  consistent with it hitting the same failure. Both tools agree; ours is
  merely louder.

  NOT established: what trace stream 6 is (the numbering is not obviously the
  fetch streamRef space - fetch ref 6 is the 16x16 baseline row and answers
  fine under FORCE_LOAD); a definitive rule for `sample`'s three silent
  textures; and whether `MTLREPLAYER_*` variables propagate to `gpudebug`'s
  XPC-hosted replayer at all (untested). Also still unpulled: the old
  observation that the same corpus sweep returns 182 records at small
  request sizes and 180 at large ones; the two repeat answers (see the
  non-uniqueness fact above) collapsing at large sizes remains the untested
  explanation.

  The discarded lead from earlier the same day stands discarded: the
  `*.gputrace.gpuprofiler_raw` correlation was an artifact of both real
  captures being Xcode exports, and is not replay input.
- **The capture-bundle shape requirement is inferred, not documented.**
  `-load:error:` SIGSEGVs on a missing path or a non-capture directory rather
  than returning an error. The prior project validated that an `index` entry
  and a `metadata` entry exist before calling `load:`, and both are present on
  every capture available to this campaign, but nothing establishes that as
  the actual, complete format requirement enforced by the framework. This
  crate's `guard` module encodes the same check as a precondition (see
  `docs/HANDOFF.md` section 3); it is a defensive floor, not a verified
  specification of what `load:` requires.

## Live probes

- **Probe:** `smoke` (`probes/src/bin/smoke.rs`).
  **EXPECTATION** (written before running): a natural-size sweep, streamRef
  0..=2000, plane 0, over `captures/small.gputrace`, should return records
  for the subset of `small.gputrace`'s 7 textures that answer the replayer.
  Per `captures/README.md`, that subset is 4 of 7, all `BGRA8Unorm`
  (`MTLPixelFormat` 80).
  **RESULT** (MEASURED 2026-09-01, macOS 27.0, controller-gated live run):
  4 records parsed, pixel-format histogram `{80: 4}`. Run completed in 2
  seconds. `pgrep -f GPUToolsReplayService` showed no orphaned process
  afterward (replayer clean). This matches the expectation exactly, with no
  surprises: same count (4), same and only format (80 = BGRA8Unorm). This is
  the first live validation of the full path bootstrap -> load -> fetch ->
  parse against the real framework on this crate, not just against fixture
  bytes.

## Status

- `GTReplayFetchTexture` - **Established.** Struct layout confirmed from the
  live runtime; fetch parameters (`depth`, region semantics, sparse
  streamRefs) confirmed by measurement; end-to-end fetch validated live by
  the `smoke` probe on 2026-09-01. Format/shape coverage completed 2026-09-02:
  depth/stencil/combined/planar/mip/array/compressed all serve as native
  single-aspect 2D images; `setSlice`/`setLevel` select slices/levels; 3D
  z-plane addressing is the one unexposed gap (see "Texture format & shape
  coverage").
- `GTMTLReplayService` (`-fetch:` path) - **Established.** Same live-smoke
  validation as above exercises `-fetch:` directly.
- `GTReplayRequestBatch` - **Established** for the two setters this crate
  uses (`-setRequests:`, `-setCompletionHandler:`); no other API surface on
  this class has been exercised.
- `GTReplayUnarchiver` - **Unverified.** Never probed. The crate currently
  parses replies by hand (`reply` module) rather than through this class; it
  is unknown whether this class is the framework's own reply decoder or
  something else entirely.
