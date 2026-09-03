# Surface: harvester

Symbols:

- `GTHarvesterGetData` (C symbol) - 2 args (block ptr, buffer size), Established
  from body; returns the data payload after the metadata.
- `GTHarvesterGetMetadata` (C symbol) - 2 args (block ptr, buffer size),
  Established from body; validating cast to the typed metadata header.
- `GTHarvesterGetTexturePlane` (C symbol) - 2 args (metadata block, plane
  index), Established from body; returns a 0x30-byte plane descriptor. Bears on
  the `plane:` open question in [00-texture-fetch.md](00-texture-fetch.md).
- `GTHarvesterGetTexturePlaneCount` (C symbol) - 1 arg (metadata block),
  Established from body; returns the plane count.

## Static findings

Disassembly of `GPUToolsReplay` via `ipsw macho disass`. Arg COUNT off the
prologue/body is Established; roles/semantics are Inferred with evidence. The
four exported symbols plus four internal helpers (`_GTHarvesterInitMetadata`,
`_GTHarvesterGetMetadataSize`, `_GTHarvesterGetTextureMetadataRequiredSize`,
`_GTHarvesterAddTexturePlane`) together define a self-contained read/write API
over an in-memory metadata block. There is **no ObjC object and no session
handle**: the "handle" is a raw `(pointer, byte-length)` pair for a "capture"
block.

### The "capture" block layout (Established from InitMetadata + all getters)

- `[0x00]` u64 magic. Written by `_GTHarvesterInitMetadata` @ 0x24faf2294 and
  checked by every getter. Constant built by the movk chain (low16=0x7265, then
  0x7475<<16, 0x6170<<32, 0x0063<<48): in-memory bytes `65 72 75 74 70 61 63 00`,
  the four halves "er","ut","pa","c\0" = the ASCII tag "capture" (byte-reversed).
- `[0x08]` u16 version. Init writes 2 (`mov w8,#2; strh w8,[x21,#8]`). Getters
  add a 0x10 header to the metadata size when version==1.
- `[0x0a]` u16 type tag. Init writes its arg1 (x1). 1 == texture (checked by the
  plane getters `cmp #1`).
- `[0x0c]` u32 metadataSize. Init writes its arg2 (x2).
- `[0x10]` u64 planeCount (texture type). AddTexturePlane increments it.
- `[0x18 + i*0x30]` plane entry i, 0x30 (48) bytes each.

### The 0x30-byte plane descriptor (DECODED 2026-09-01, ground truth)

Six little-endian `u64` fields, validated against the known-textures fixture
(each texture's dimensions/format are known):

| offset | field | example (64x64 BGRA) |
| --- | --- | --- |
| `0x00` | pixelFormat (MTLPixelFormat) | 80 (BGRA8Unorm) |
| `0x08` | width | 64 |
| `0x10` | height | 64 |
| `0x18` | depth | 1 |
| `0x20` | bytesPerRow | 256 (= width*4) |
| `0x28` | bytesPerImage (= size) | 16384 (= width*height*4) |

Confirmed on three textures (64x64, 80x64, 112x112): `bytesPerRow = width*4`,
`bytesPerImage = width*height*4` in every case.

### Real blocks: the bundle content files ARE capture blocks

The capture bundle's `MTLTexture-N-...` files begin with the "capture" magic
(version 2, type 1 texture, `metadataSize` 0x100, `planeCount` 1, the plane
descriptor at 0x18, then the pixel data at 0x100). So a REAL harvester block is
just a resource content file - no need to reach `harvestTexture`. The
framework's own getters parse them correctly (MEASURED, `probes/run.sh
harvestreal <MTLTexture-file>`): `GetMetadata` accepts them, `GetTexturePlaneCount`
returns 1, `GetTexturePlane(0)` yields the descriptor above, and `GetData`
returns `block+0x100` (the pixels).

### Exported getters

- **`_GTHarvesterGetMetadata` @ 0x24faf2310: 2 args (Established).** x0 = block
  pointer, x1 = buffer size in bytes. Returns x0 iff `x0!=0 && x1>=0x10 &&
  [x0]==magic`, else 0. A validating cast: `(buffer, len) -> metadata header`.
- **`_GTHarvesterGetData` @ 0x24faf2344: 2 args (Established).** x0 = block, x1 =
  buffer size. Same validation, then returns `x0 + metadataSize` (metadataSize =
  `[x0+0xc]`, +0x10 when version==1). So the data payload sits immediately after
  the metadata inside the same buffer.
- **`_GTHarvesterGetTexturePlaneCount` @ 0x24faf239c: 1 arg (Established).**
  x0 = metadata block. If `[x0+0xa]`(type)==1 returns `[x0+0x10]` (planeCount),
  else 0.
- **`_GTHarvesterGetTexturePlane` @ 0x24faf23bc: 2 args (Established).**
  x0 = metadata block, x1 = plane index. If type==1 returns
  `x0 + 0x18 + index*0x30`, else 0; also 0 when index > planeCount (the bound
  check is `count < index -> 0`, so index==count is not rejected, a likely
  off-by-one; treat count as the exclusive bound). Each entry is a 0x30-byte
  plane descriptor whose internal fields are not yet decoded.

### Internal helpers (the producer side)

- **`_GTHarvesterInitMetadata` @ 0x24faf2294: 3 args (Established:
  x2->x19, x1->x20, x0->x21).** (block, u16 type, u32 metadataSize). Zeroes the
  block (bzero/memset with size=x2), writes magic/version=2/type/size, returns x0.
- **`_GTHarvesterGetMetadataSize` @ 0x24faf22f4: 1 arg.** Returns `[x0+0xc]`
  (+0x10 if version==1); 0 if x0==0.
- **`_GTHarvesterGetTextureMetadataRequiredSize` @ 0x24faf2388: 1 arg
  (plane count).** Returns `(0x30*count + 0x117) & ~0xff`: 0x30 per plane plus a
  header, rounded up to 256 bytes. This is the allocation size for an n-plane
  texture block.
- **`_GTHarvesterAddTexturePlane` @ 0x24faf23f0: 2 args.** (block, pointer to a
  0x30-byte source plane descriptor). Copies 0x30 bytes to
  `block+0x18+planeCount*0x30`, then increments planeCount at `[block+0x10]`.

### How a block is produced (answers "how do we drive the harvester")

The block is built internally by `_GTMTLReplayClient_harvestTexture` @
0x24f88cf44 (an internal `t` symbol taking 8+ args), which calls
`_GTHarvesterGetTextureMetadataRequiredSize` (0x24f88d81c, 0x24f88d92c),
`_GTHarvesterInitMetadata` (0x24f88d944, type=texture), and
`_GTHarvesterAddTexturePlane` (0x24f88d9fc) once per plane. The product is the
"capture" block (header + plane descriptors) followed by the raw texture data.
The exported Get* functions are the consumer side, all keyed off `(buffer,
size)`: `GetMetadata` validates and returns the header, `GetTexturePlaneCount` /
`GetTexturePlane` walk the descriptors, `GetData` returns the payload after the
metadata. So the harvester object/handle is not obtained via these functions at
all; it is a memory blob the replay client emits and the getters parse.

- These are C symbols, not ObjC classes, so no `GTHarvester*` ObjC class appears
  in `docs/findings/raw/classdump-27.txt`. (There is an unrelated
  `GTMTLReplayActivityHarvestResourceObject` ObjC class, and internal
  `_HarvestTensorPlane` / `_HarvestTileImageBlockMemory` routines, but the Get*
  API operates only on the raw block above.)

## Open questions

- RESOLVED (static): the first argument is not a session/controller handle. It
  is a raw pointer to an in-memory "capture" metadata block, paired with a
  buffer-size argument (for GetMetadata/GetData). The block is emitted by
  `_GTMTLReplayClient_harvestTexture`, not by these functions.
- RESOLVED (2026-09-01): the 0x30-byte plane descriptor is six `u64`s -
  pixelFormat, width, height, depth, bytesPerRow, bytesPerImage - decoded by
  ground truth against the known-textures fixture (see above), not by
  disassembly.
- RESOLVED (2026-09-01): a real `(buffer, size)` capture block is simply a
  capture-bundle `MTLTexture-N-...` content file - it begins with the "capture"
  magic and the framework's getters parse it directly. No need to reach the
  internal `harvestTexture`.
- Whether `GTHarvesterGetTexturePlane` / `GTHarvesterGetTexturePlaneCount`
  expose a genuinely different view of plane data than `GTReplayFetchTexture`'s
  `-setPlane:` is now clarified in principle: these read the plane descriptors of
  an already-harvested block, so they are the read-out side of the harvest path,
  not a live re-fetch. Whether that resolves the `plane:` open question in
  [00-texture-fetch.md](00-texture-fetch.md) still depends on obtaining a block.
- Whether "harvester" is a bulk/offline extraction path distinct from the
  interactive fetch path (`GTReplayFetchTexture` + `-fetch:`) remains open, but
  the block format (magic "capture", versioned, self-describing) reads like a
  serialization/interchange format rather than an in-process fetch buffer.

## Live probes

**All four getters confirmed live (2026-09-01)** by a ground-truth round-trip
(`crates/gputools-replay-sys` test `the_harvester_getters_parse_a_synthetic_capture_block`):
a synthetic "capture" block built to the layout above (magic, version 2, type 1,
metadataSize 0x78, planeCount 2, two 0x30-byte plane descriptors, then a data
payload) is passed to the framework's own getters, which return exactly what the
layout predicts:

- `GTHarvesterGetMetadata(block, size)` returns the block when valid; null for
  `size < 0x10`, a null block, or a wrong magic.
- `GTHarvesterGetTexturePlaneCount(block)` returns 2.
- `GTHarvesterGetTexturePlane(block, i)` returns `block + 0x18 + i*0x30`, and the
  bytes there are the descriptor we wrote.
- `GTHarvesterGetData(block, size)` returns `block + metadataSize`, i.e. our
  payload, byte-for-byte.

So the four exported symbols and the block layout are behaviour-confirmed
against the live framework, no replayer session needed (they are pure
block parsers). Still open: obtaining a REAL block from `harvestTexture` (an
internal `t` symbol) and decoding the 0x30-byte plane descriptor's internal
fields.

## Status

- `GTHarvesterGetData` - **Behavior confirmed (2026-09-01) by round-trip:
  returns block+metadataSize (the payload).** 2 args (block, buffer size).
- `GTHarvesterGetMetadata` - **Behavior confirmed (2026-09-01): validates and
  returns the "capture" header; rejects too-small/null/bad-magic.** 2 args.
- `GTHarvesterGetTexturePlane` - **Behavior confirmed (2026-09-01): returns
  block+0x18+index*0x30.** 2 args (block, plane index). Descriptor field layout
  still undecoded.
- `GTHarvesterGetTexturePlaneCount` - **Behavior confirmed (2026-09-01):
  returns planeCount `[block+0x10]` for a texture block.** 1 arg.
