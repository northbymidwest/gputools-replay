# Fixture Apps

Each fixture app is a tiny standalone Metal program that renders exactly what
one probe needs, with ground-truth values baked in. For example, the YCbCr app
writes a distinct gradient per plane so the `plane:` probe reads which plane
came back rather than inferring it.

## Building and Capturing

Each fixture app is built by a plain `clang` or `swiftc` line, with no Xcode
project or build system. To capture a fixture:

```bash
fixture-apps/capture.sh <app-binary> <output.gputrace>
```

The script wraps `gpucapture(1)`, setting `MTL_CAPTURE_ENABLED=1` and
`MTLCAPTURE_WAIT_FOR_SIGNAL=1` so the trace is deterministic.

## Fixture Apps

- `known-textures.m` (**written**): six textures with exact ground truth - a
  distinct width per row so a reply record identifies its row, and a distinct
  solid clear colour per row so a fetched payload can be checked pixel for
  pixel. The rows form a property matrix, each varying ONE thing from the
  baseline: storage mode (Private vs Shared), usage (RenderTarget vs
  RenderTarget|ShaderRead), whether anything reads the texture (it is a blit
  source), how it is written (render-pass Clear vs blit destination vs CPU
  `-replaceRegion:`). Build and capture:

  ```bash
  clang -fobjc-arc -fmodules -O0 -o /tmp/known-textures \
        fixture-apps/known-textures.m -framework Metal -framework Foundation
  fixture-apps/capture.sh /tmp/known-textures captures/known-textures.gputrace
  ```

  Two-phase mode: with `KNOWN_TEXTURES_GO_FILE=<path>` set, the app creates
  and fills the six textures, blocks until that file exists, then creates a
  SEVENTH (`late_created`, w=112) and clears it. `capture-late.sh` drives that
  so the capture boundary falls between the phases:

  ```bash
  fixture-apps/capture-late.sh /tmp/known-textures captures/known-textures-late.gputrace
  ```

  **Results (2026-09-01):** by default none of the textures is fetchable, by
  our path or by `gpudebug`. They are "unused resources" - the captured
  commands never read them - and `MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE=1`
  makes all of them answer (0 -> 7). Full table in
  `docs/findings/00-texture-fetch.md`.

- `accel-structure.m` (**written**): builds one primitive `MTLAccelerationStructure`
  over a triangle (needs `supportsRaytracing`). Two-phase (`FIXTURE_GO_FILE`):
  build, block for the boundary, then refit inside the capture so the structure
  pre-exists AND is used. Capture with `capture-late.sh`; the first
  acceleration-structure capture in the campaign. Fetched live via
  `probes/run.sh rawfetch GTReplayFetchAccelerationStructure`; see dossier 05.
  Env `ACCEL_VERTS` (nine comma-separated floats) overrides the triangle, which
  is how the fetched byte format was decoded by controlled variation.
- `known-buffers.m` (**written**): ground truth for the BUFFER, HEAP and
  PIPELINE fetch classes. Three standalone buffers with distinct sizes and
  arithmetic patterns (`in_a[i]=i` 256 B, `in_b[i]=0x1000+i` 384 B, a GPU-written
  `out` 512 B), plus one heap-allocated buffer (`0x2000+i` 256 B), all read or
  written by one compute kernel - so the buffers, the heap, AND the compute
  pipeline are used resources. Two-phase (like `accel-structure.m`): a
  single-phase `capture.sh` run answers nothing, because resources created and
  destroyed inside one capture are not snapshotted for fetch; the late boundary
  makes them pre-exist it. Build and capture:

  ```bash
  clang -fobjc-arc -fmodules -O0 -o /tmp/known-buffers \
        fixture-apps/known-buffers.m -framework Metal -framework Foundation
  fixture-apps/capture-late.sh /tmp/known-buffers captures/known-buffers.gputrace
  ```

  Verified by `crates/gputools-replay/tests/live_buffers.rs`: `fetch_buffers`
  returns the four buffers byte-for-byte, `fetch_heaps` returns the 64 KB heap
  containing the sub-buffer's pattern, and `fetch_pipeline_binaries` returns the
  compute pipeline as a nested bplist of a Mach-O binary (dossier 03).

- `known-draws.m` (**written**): ground truth for the dispatch-keyed WIREFRAME
  fetch class (GTReplayFetchWireframe). One render pass issues three separate
  `drawPrimitives` triangle draws into a 256x256 target; wireframe fetch is
  keyed by dispatchUID (a command-stream draw index, not a streamRef), so each
  draw can be fetched as a rendered wireframe image. Two-phase, like
  known-buffers. Build and capture:

  ```bash
  clang -fobjc-arc -fmodules -O0 -o /tmp/known-draws \
        fixture-apps/known-draws.m -framework Metal -framework Foundation
  fixture-apps/capture-late.sh /tmp/known-draws captures/known-draws.gputrace
  ```

  Verified by `crates/gputools-replay/tests/live_draws.rs`: `fetch_wireframes`
  over dispatchUIDs 4..=8 returns five 256x256 R8 images, at least the three
  triangle draws non-blank. NOTE: requesting a dispatchUID that is not a real
  draw makes the replayer raise an internal error, which the safe crate
  surfaces as `FetchError::Replayer` (it has no tolerate-errors hatch), so the
  test requests only the indices the fixture produces.

- `known-depth.m` (**written**): ground truth for DEPTH texture fetch (never
  exercised before the gputools-replay-hl design). Renders a full-screen
  triangle at a known constant depth (0.5) into a `Depth32Float` attachment,
  then blits it to a second depth texture so the rendered content is
  snapshotted (a write-only render target is not stored - measured). Two-phase.
  Build and capture:

  ```bash
  clang -fobjc-arc -fmodules -O0 -o /tmp/known-depth \
        fixture-apps/known-depth.m -framework Metal -framework Foundation
  fixture-apps/capture-late.sh /tmp/known-depth captures/known-depth.gputrace
  ```

  MEASURED: the blit-source depth texture fetches via `GTReplayFetchTexture` as
  `fmt=252` (`MTLPixelFormatDepth32Float`), one f32 per pixel, standard
  `bytes_per_row`, reading exactly `0.5` everywhere. So depth textures fetch as
  their native single-channel type through the ordinary texture path.

- `known-depth-stencil.m` (**written**): characterizes COMBINED depth+stencil
  fetch. Renders at depth 0.5 and writes stencil reference 42 into a
  `Depth32Float_Stencil8` attachment (blit-stored), two-phase. MEASURED: the
  combined texture fetches via `GTReplayFetchTexture` as `fmt=252`
  (`Depth32Float`, 4 bytes/pixel) - the DEPTH ASPECT ONLY (reads 0.5); the
  stencil aspect is not surfaced. So combined depth/stencil decomposes to
  depth-only on fetch; no compound layout to decode.

  ```bash
  clang -fobjc-arc -fmodules -O0 -o /tmp/known-depth-stencil \
        fixture-apps/known-depth-stencil.m -framework Metal -framework Foundation
  fixture-apps/capture-late.sh /tmp/known-depth-stencil captures/known-depth-stencil.gputrace
  ```

- `known-ycbcr.m` (**written**): investigates PLANAR / YCbCr texture fetch. A
  64x64 biplanar 4:2:0 `CVPixelBuffer` (Y=128, Cb=100, Cr=150) wrapped as
  per-plane MTLTextures and sampled in a compute pass, two-phase. MEASURED: the
  planes fetch as two SEPARATE ordinary texture records - luma `R8Unorm` 64x64
  (all 128), chroma `RG8Unorm` 32x32 (Cb 100 / Cr 150) - each its own streamRef,
  byte-exact via the normal path. So planar textures are just per-plane R8/RG8
  textures; no plane parameter needed for the CVPixelBuffer case. Needs
  `-framework CoreVideo`.
- `known-mips.m` (**written**): MIPMAP + ARRAY-slice fetch. A 2D-array (slice0
  red, slice1 green) mipmapped BGRA8Unorm texture, mips generated + blit-stored.
  MEASURED: `GTReplayFetchTexture` has `-setSlice:`/`-setLevel:` (which the
  substrate does not yet call); wiring `setSlice` returns slice 0 = red, slice 1
  = green exactly. So array slices and mip levels ARE addressable by index (the
  default fetch just returns slice 0 / level 0).
- `known-astc.m` (**written**): investigates COMPRESSED (ASTC) fetch. A 64x64
  `ASTC_4x4_LDR` texture filled with a known 16-byte block pattern
  (0x00..0x0F), blit-stored. MEASURED: fetch returns the RAW COMPRESSED blocks
  byte-for-byte (`fmt=204`, 4096 B = 256 blocks x 16, `bytes_per_row`=256); the
  replayer does NOT decompress. Compressed textures expose their blocks;
  decompression is downstream.
- `known-stencil.m` (**written**): investigates STENCIL fetch. Renders stencil
  42 into a base `Stencil8` texture and stencil 77 + depth 0.5 into a combined
  `Depth32Float_Stencil8` (with an `X32_Stencil8` view), blit-stored. MEASURED:
  base `Stencil8` fetches as `fmt=253` (1 B/px, value 42); the combined format
  surfaces as separate resources - depth aspect `Depth32Float` (0.5) and a
  stencil-view aspect `X24/X32_Stencil8` (1 B/px). Stencil reads as `u8`; the
  combined `fmt=260` is never returned directly. (Stencil-view value storage is
  capture-dependent.)
- `known-3d.m` (**written**): 3D (volume) fetch. A 16x16x4 BGRA8Unorm volume,
  z-slices blue 10/20/30/40, blit-stored. MEASURED: unlike array slices, no
  parameter (`setSlice`, `setLevel`, `region.origin.z`, `plane`) selects a 3D
  z-plane - every fetch returns one fixed plane. `GTReplayFetchTexture` has no
  `-setDepthPlane:`. So 3D z-plane addressing is not exposed by the fetch API
  (a documented gap); array/mip ARE (see known-mips).
- `known-ambiguous.m` (**written**): resolves the intra-dims-run ordering for
  the bundle descriptor join (see `docs/findings/00-texture-fetch.md`). Three
  64x64 BGRA8 textures sharing dims+format but with distinct mip counts and
  distinct clear colours (red/mip1, green/mip3, blue/mip7), so the fetched
  pixels identify each physical texture and construction pins colour->mip.
  MEASURED: fetch-streamRefs ascending (2,3,4 = red,green,blue) map rank-for-
  rank to descriptors sorted by store0 offset (505,575,666 = mip 1,3,7), so
  `streamRef-rank == store0-offset-rank` holds WITHIN a run, not just across
  runs. The join may attribute descriptors inside an ambiguous run.
- `known-ds-pair.m` (**written**): resolves how COMBINED depth-stencil maps
  manifest-descriptor -> fetched-aspect for the bundle descriptor join. Two
  combined `Depth32Float_Stencil8` resources at distinct dims (64x64 + 96x96)
  with distinct depth/stencil values (64x64 = 0.25/11, 96x96 = 0.75/22). MEASURED:
  a combined resource is ONE manifest descriptor (format 260) and ONE fetched
  streamRef; plane 0 serves the depth aspect (252), plane 1 the stencil aspect
  (261), on the SAME streamRef (1:1, not 1:2); content is byte-exact per aspect
  (depth 0x0000803e/0x0000403f, stencil 0x0b/0x16). Only the format diverges
  (260 vs 252/261). Conclusion: the join matches on exact `(w,h,format)` and
  treats combined depth-stencil descriptors as transparent (their aspects are
  honest raw fetches, not reassembled in v0). See
  `docs/findings/00-texture-fetch.md`. FIXTURE PATTERN (learned the hard way):
  allocate every texture ONCE and re-render into the same objects in phase 2 -
  allocating fresh each phase leaves the captured resource with uninitialised
  content (NaN depth / 0xFF stencil) and can make it unfetchable.
- `raytrace` (planned): acceleration structures

## Correctness probes

- `probes/run.sh pixelcheck <trace>` - compares fetched pixels against the
  fixture's ground-truth colours; asserts the fetch+decode path is byte-correct
  on stored-content textures.
- `probes/run.sh datadiff <trace>` - per-streamRef, whether `play_all()`
  changes a texture's fetched bytes. Used to show real captures preserve used
  content across playback.
- `probes/run.sh texbmp <trace> <ref> <out>` - writes one fetched texture as a
  BMP (+ raw BGRA), before and after `play_all()`, for the oracle comparison.

Oracle pixel comparison (validated the fetch path against `gpudebug` on a real
texture): `texbmp` writes ours; `gpudebug` fetches the same `texN` to a PNG
(under its own temp dir, not `-o`); `sips -s format bmp` converts the PNG for
parsing; a small script compares RGB. The images match once `gpudebug`'s
Generic-RGB display gamma (best-fit `ours ** 0.84`, tagged in the PNG per
`sips -g profile`) is accounted for - our raw bytes are the untransformed
truth.

## gpudebug(1) as an oracle

`gpudebug -t <trace> -o <dir>` is Apple's own implementation of the path this
project reimplements, and it is the fastest way to tell "our fetch is broken"
apart from "this resource is not fetchable". It is an interactive REPL, so
drive it by piping commands:

```bash
printf 'go resources\ngo textures\nlist\ninfo tex0 --all\nfetch tex0\nwait\n' \
  | gpudebug -t captures/<trace>.gputrace -o /tmp/out --oneshot --timeout 90
```

`info <node> --all` prints the `resourceIndex`, which is the streamRef our
fetch path takes. NOTE: gpudebug drives replay over XPC, the path that can
lock the replayer for two hours - always pass `--oneshot` and `--timeout`.

Apps and scripts are committed to the repository. Captures land in `captures/`,
which is gitignored.

## Important Warnings

**Access must be serialized.** One replay session at a time, machine-wide.

**An interrupted run orphans a session and locks the replayer for TWO HOURS.**
If a capture hangs or is interrupted, recover with:

```bash
gpudebug --terminate all
pkill -9 -f GPUToolsReplayService
```

Check `pgrep -f GPUToolsReplayService` before and after every run.

**Latency ranges from ~27 seconds to 20+ minutes.** Do not assume a slow run
hung.
