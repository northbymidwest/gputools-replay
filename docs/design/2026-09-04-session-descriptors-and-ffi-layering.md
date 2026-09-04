# Session-based texture descriptors, and consolidating the FFI into `-sys`

Status: proposed (2026-09-04); core mechanism VALIDATED LIVE (2026-09-04) by
the `objectmap` probe. No production code yet.

## Live validation (probe `objectmap`, 2026-09-04)

Confirmed in-process on both repro captures, and it resolves both open
questions below:

- The object map is a **direct field of the controller at `controller + 0x8`**
  (the controller is the C struct at client field 1; the map is
  `*(controller + 0x8)`), stable across both captures. It is NOT in the ObjC
  service graph (`GTMTLReplayService` has only `_observers`); the probe found
  it by a `malloc_size`-guarded scan of the controller's heap.
- `tryGetTextureForKey:(streamRef)` returns live `AGXG13XFamilyTexture`s with
  correct public Metal properties. **SDL3** `vibeboy.gputrace` (which
  gputrace-bundle reads as 0 descriptors): streamRefs 24-27, all
  `2880x2592 fmt=80 (BGRA8Unorm) type=2 mips=1 array=1` - 4 textures, matching
  the consumer's "4 fetch fine." **winit**: streamRefs 21-23
  (`1440x1296 fmt=70`, two `2880x2756 fmt=80`) - 3 textures, which is exactly
  finding #1's "3 of 5 answer without FORCE_LOAD."
- So the map holds the **loaded** resources (unused ones absent without
  FORCE_LOAD), the accessor is a fixed struct offset, and descriptors come
  authoritatively off live Metal objects - size-agnostic, streamRef-keyed, no
  join. The design is proven end to end.

## Problem

A consumer reported (against `gputools-replay-ktx2-fetch` 0.1.2) that
`gputrace-bundle` finds no descriptors in an SDL3 `.gputrace`
(`manifest_status() == NoDescriptors`) even though the textures fetch fine,
while a wgpu/winit capture of the same app parses (5 descriptors). Repro:
`vibeboy.gputrace` (SDL3) vs `vibeboy_winit.gputrace` (wgpu).

Measured: the SDL3 texture descriptors are 240 bytes; the parser hardcodes 248.
The obvious "also accept 240" fix is a second magic number, so we asked the
real question instead: how does the official tooling identify a texture
descriptor, and where should a correct reader get one?

## What the reverse engineering established (MEASURED)

Chased from `gpudebug` through the framework binaries
(`otool`/`nm`/`ipsw`/`ipsw macho disass`):

- `gpudebug` is `GPUToolsFoundationCLI`; it drives a `.gputrace` through the
  **replayer** (`GPUToolsReplay.framework`), which is the framework this stack
  already links via `gputools-replay-sys`.
- **The trace is a stream of recorded Metal API calls.** The replayer has a
  `_DYTraceDecode_<Class>_<method>` function per captured call
  (`_DYTraceDecode_MTLDevice_newTextureWithDescriptor`,
  `_MTLHeap_newTextureWithDescriptor`, the `newTextureView*` family, ...) and
  event keys `kDYFE...` ("DY Function Event"). Loading a trace decodes and
  re-executes each call. **A resource's type comes from the call that created
  it** (`newTextureWithDescriptor` makes a texture), not from any field of the
  stored record.
- **The store is untyped.** `store0` is a `GTChunkTable` (content-addressed,
  keyed by streamRef); `index` is `xdic` (an APR hash table used for alias
  resolution and 16-hex per-record keys); `metadata` is a `DYCaptureSession`
  keyed archive (session env + counts, not a per-resource type graph).
  `GTTraceContext_openStream(ctx, object, self)` takes no type argument
  (decompiled: it mints a stream id with an atomic counter and stores the
  object pointers). A trace stream is a generic `(unique id, bytes)` container;
  the owning `CaptureMTL*` class is never recorded.
- The descriptor struct's word 0 (winit `120`, SDL3 `115`) is shared by *every*
  structured object in a capture, at all sizes, so it is a **per-capture
  serialization schema version**, not a type. 248 vs 240 is **trailing zero
  padding** (winit's word 30 is `0`; words 24-29 are zero in both), not a
  schema difference in the meaningful fields.

Conclusion: the on-disk store does not statically type its records, so any
session-free reader (ours, and `gpu-trace-parse-rs`) is forced into
heuristics - the `size == 248` test, and the ordinal join
(streamRef-rank <-> store0-offset-rank). The authoritative typing lives in the
event stream, which the replayer decodes.

- **The replayer exposes the loaded objects.** `GTMTLReplayObjectMap` is a
  streamRef-keyed registry of the live, fully typed Metal objects the load
  created: `-textureForKey:`, `-bufferForKey:`, `-allocationForKey:`,
  `-accelerationStructureForKey:`, `-resources` (enumerate), and
  `-addUnusedResourceKey:` (used/unused tracking).

## Decision 1: descriptors become session-based

Read descriptors from `GTMTLReplayObjectMap`: `textureForKey:(streamRef)` ->
live `MTLTexture` -> its public Metal properties
(`width`/`height`/`depth`/`pixelFormat`/`textureType`/`mipmapLevelCount`/
`arrayLength`/`sampleCount`/`usage`). This is the same mechanism `gpudebug`
uses, so it is correct on SDL3 and every schema by construction.

It retires, at once: the `size == 248`/`240` heuristic, the schema-tag guess,
the descriptor field-map RE, and **the ordinal join** - fetch is keyed by
streamRef and so is the map, so a fetched texture and its descriptor share the
key with no rank-matching.

Tradeoff, accepted: descriptors now require a live session (macOS 27 + the
framework), the opposite of `gputrace-bundle`'s session-free promise. This is
acceptable because a descriptor is wanted alongside a fetch of its pixels, and
fetch already needs a session; a consumer that fetches always holds one.

## Decision 2: all raw framework FFI lives in `-sys`

Today the ObjC bindings (`extern_class!`/`extern_methods!` for the `GTReplay*`
request/batch/service classes) live in `crates/gputools-replay/src/objc.rs`
(the mid, "safe" crate). That placement was incidental: its own module doc
gives a DRY rationale ("one typed source of truth instead of ad hoc
`msg_send!`"), colocated with its consumer `fetch.rs` - never a layering
decision.

From first principles it belongs in `-sys`. In the objc2 ecosystem the
bindings crate *is* where `extern_class!` lives - `objc2-metal` and
`objc2-foundation` are exactly that, and you wrap them safely above. There is
no principled basis for "C signatures in `-sys`, ObjC classes in mid"; both are
equally "what the framework dictates," which is `-sys`'s stated charter. And
the goal of mapping and exposing the full framework makes `-sys` the natural
home: a complete raw binding surface, colocated with the `inventory` module
that already enumerates those classes and regression-tests their existence,
growing under the campaign's established-only discipline (a class graduates
into `-sys` once its shape is measured).

Resulting layers:

- **`gputools-replay-sys`**: every established binding - the C functions, the
  client ABI, the `GTReplay*` classes (migrated from mid), `GTMTLReplayObjectMap`
  (new), and whatever RE surfaces next - plus the inventory. Gains an
  `objc2-metal` dependency for the object map's `id<MTLTexture>` return types.
- **`gputools-replay`** (mid): purely safe. `Session`, fetch, playback, and the
  new descriptor read; imports typed classes from `-sys`. The only raw sends
  that reasonably stay are genuinely policy, not bindings (e.g. the
  `-respondsToSelector:` NSObject-protocol guard in `read_response`).
- **`gputools-replay-hl`**: domain interpretation (`FormatKind`,
  `MTLPixelFormat`, `packed_bytes`, `Descriptions`), unchanged, now fed from
  the session.

## The seam

A raw-numeric texture descriptor keyed by streamRef - the same shape hl already
interprets from `gputrace-bundle::TextureDescriptor`, so hl's logic is reused
and only the source changes. mid reads the live `MTLTexture`'s properties and
exposes them as `u32`/`u64` (holding the "formats stay numeric; enums live in
hl" line the crate already draws); hl interprets.

## Plan (phased, each independently reviewable)

1. **Migrate the ObjC FFI mid -> `-sys`** (pure refactor, no behavior change).
   DONE 2026-09-04 (commits 179245c + d4a177c, CI green). `objc.rs` became
   `-sys/src/replay.rs` (all `extern_class!`/`extern_methods!`/`StreamRefFetch`,
   now `pub`); the wire-format param structs (`GTSize`/`GTPoint3D`/`GTRegion`/
   `DispatchUid`) moved to `-sys` too; the domain types and `Region::to_gt`
   (replacing the orphan-violating `From<Region> for GTRegion`) stayed in mid;
   `new_request` (policy) moved into `fetch.rs`. `-sys` gained `objc2-foundation`
   + `block2`.
2. **Bind `GTMTLReplayObjectMap`** and the `Session` -> map accessor in `-sys`.
3. **mid**: `Session::texture_descriptor(stream_ref) -> Option<TextureDescriptor>`
   (raw numeric, read from the live object) and a loaded-resource enumerator.
4. **hl**: source descriptors from the session; retire the ordinal join for
   session consumers; `manifest_status` from the map.
5. **`gputrace-bundle`**: demote to an optional offline heuristic reader, no
   longer the descriptor source on the session path. (This is where consumer
   finding #5 stops mattering for real consumers.)

## Open questions to settle during implementation

- **`Session` -> `GTMTLReplayObjectMap` accessor: RESOLVED (2026-09-04).** The
  map sits at `controller + 0x8` (the controller being the C struct at client
  field 1), confirmed live on both captures. It is a `GTIntKeyedDictionary`
  keyed by streamRef, populated during load by the `_DYTraceDecode_*` call
  decoders, with the full family (`textureForKey:`, `tryGetTextureForKey:`,
  `setTexture:forKey:` and siblings, `resources`). Not an ObjC ivar of the
  service (otool finds no such typed ivar; it is a controller struct field). So
  `-sys` reads `*(controller + 0x8)` for the map. NOTE: 0x8 is a struct offset
  in a private type; treat it like `CONTROLLER_OFFSET` in client.rs (measured,
  regression-guarded), and consider also probing `-[GTMTLReplayObjectMap ...]`
  via runtime once the map is in hand rather than trusting the offset forever.
- **Unused resources: resolved (2026-09-04).** `textureForKey:` and
  `tryGetTextureForKey:` are pure dictionary lookups; neither triggers a load
  (`textureForKey:` calls `GTMTLReplay_dispatchFailedToGet` on a miss,
  `tryGetTextureForKey:` returns nil). So a resource is in the map only if the
  load path inserted it: unused resources are absent by default and present
  under `FORCE_LOAD`, matching the fetch used/unused rule. Use
  `tryGetTextureForKey:` (nil-safe). This is the lead for consumer finding #1.
- **Descriptor type identity.** mid defines its own `TextureDescriptor`; does hl
  unify it with `gputrace-bundle`'s (one canonical type both feed) or convert?
  Decides whether hl keeps a `gputrace-bundle` dependency at all.
- **Enumeration.** `-[GTMTLReplayObjectMap resources]` vs. a streamRef sweep
  bounded by `record_count`. If the map enumerates directly, `record_count`'s
  reason to exist goes away too.
- **mid return type.** Numeric descriptor (recommended, keeps a live
  `MTLTexture` out of mid's data-only API) vs. a safe live-object handle (an
  hl-level extra if ever wanted).

## Relationship to the other consumer findings

This design directly touches **#1** (the object map's unused-resource tracking
is the lead on a finer force-load knob). **#2** (fetch position: index 0 is the
stored snapshot, `gpudebug` fetches at end) and **#3** (two-phase fixtures lose
stored content after `play_all`; live tests that fetch post-playback need
content produced inside the capture) are knowledge updates for the findings
dossiers. **#4** (fetch-after-`play_all` under force-load fails on
`known-textures-late`) is still uncharacterized. None are resolved here.
