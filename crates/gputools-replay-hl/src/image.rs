//! Fetched images: [`Texture`] and [`Wireframe`], with format-aware typed
//! pixel access. Raw bytes are always available; typed access decodes on
//! demand and errors rather than reading lossily.

use crate::Error;
use crate::bytes::Payload;
use crate::format::{FormatKind, format_kind, mtl_format};
use bytemuck::Pod;
use gputools_replay::reply::{TextureRecord, WireframeRecord};
use objc2_metal::MTLPixelFormat;
use std::borrow::Cow;

/// The shared geometry + payload both `Texture` and `Wireframe` decode
/// pixels from. Not exposed directly; both handles embed one and delegate.
struct Image {
    width: u32,
    height: u32,
    bytes_per_row: u32,
    pixel_format: u32,
    payload: Payload,
}

impl Image {
    fn format(&self) -> MTLPixelFormat {
        mtl_format(self.pixel_format)
    }

    fn format_kind(&self) -> FormatKind {
        format_kind(self.pixel_format)
    }

    fn raw_bytes(&self) -> &[u8] {
        self.payload.bytes()
    }

    fn pixels<P: Pod>(&self) -> Result<&[P], Error> {
        let bpp = self.checked_bpp::<P>()?;
        let row_len = self.width as usize * bpp;
        if self.bytes_per_row as usize != row_len {
            return Err(Error::Padded {
                bytes_per_row: self.bytes_per_row,
                row_len: row_len as u32,
            });
        }
        // Cast only the geometry-implied region, not the whole payload: a
        // payload with trailing bytes beyond `height * bytes_per_row` must
        // not silently yield more than `width * height` pixels.
        let need = self.checked_len(bpp)?;
        bytemuck::try_cast_slice(&self.raw_bytes()[..need]).map_err(|_| Error::Misaligned {
            align: std::mem::align_of::<P>(),
        })
    }

    fn rows<P: Pod>(&self) -> Result<impl Iterator<Item = &[P]>, Error> {
        let bpp = self.checked_bpp::<P>()?;
        self.checked_len(bpp)?;
        let (bpr, w, h) = (
            self.bytes_per_row as usize,
            self.width as usize,
            self.height as usize,
        );
        let bytes = self.raw_bytes();
        // Each row already slices to exactly `w * bpp`, ignoring any
        // trailing payload bytes past `height * bytes_per_row`, so no
        // further capping is needed here. `checked_len` above already
        // guarantees `bytes_per_row >= w * bpp` and `bytes.len() >= h * bpr`,
        // so every row's slice is in-bounds; only the cast itself can still
        // fail, on a per-row misalignment (validated eagerly here, not
        // lazily per `next()`, so the `Result` stays on the outer call).
        let mut rows = Vec::with_capacity(h);
        for y in 0..h {
            let row = &bytes[y * bpr..y * bpr + w * bpp];
            let row: &[P] = bytemuck::try_cast_slice(row).map_err(|_| Error::Misaligned {
                align: std::mem::align_of::<P>(),
            })?;
            rows.push(row);
        }
        Ok(rows.into_iter())
    }

    // The payload must hold at least `height * bytes_per_row` bytes, or a
    // truncated payload would either panic (`rows`' row slicing, or
    // `bytemuck::cast_slice`'s `OutputSliceWouldHaveSlop` when the shortfall
    // isn't a multiple of the element size) or silently short-read fewer
    // pixels than `width * height` (when it is) - a lossy read the "raw
    // bytes always available, typed access exact or errors" contract must
    // not allow. Checked once up front so both callers fail the same way.
    // Also requires `bytes_per_row >= width * bpp` (both fields come off the
    // wire, so neither is trustworthy alone): a record reporting a shorter
    // stride than one row of pixels needs would otherwise let `rows()`'s
    // per-row slice run past the payload and panic. `pixels()` already
    // rejects `bytes_per_row > row_len` itself (as `Padded`, before calling
    // this), so this guard only ever fires for the `<` direction there; for
    // `rows()`, which has no such pre-check, it covers both.
    // Returns `need = height * bytes_per_row` on success, so `pixels()` can
    // cap its cast to exactly the geometry-implied region (guarding the
    // over-long side of the same contract: trailing payload bytes must not
    // silently inflate the pixel count).
    fn checked_len(&self, bpp: usize) -> Result<usize, Error> {
        let row_len = (self.width as usize)
            .checked_mul(bpp)
            .ok_or(Error::Truncated)?;
        if (self.bytes_per_row as usize) < row_len {
            return Err(Error::Truncated);
        }
        let need = (self.height as usize)
            .checked_mul(self.bytes_per_row as usize)
            .ok_or(Error::Truncated)?;
        if self.raw_bytes().len() < need {
            return Err(Error::Truncated);
        }
        Ok(need)
    }

    fn blocks(&self) -> Result<Blocks<'_>, Error> {
        match self.format_kind() {
            FormatKind::Compressed(c) => Ok(Blocks {
                bytes: self.raw_bytes(),
                block: c.block,
                block_bytes: c.block_bytes,
                blocks_per_row: (self.bytes_per_row as usize) / c.block_bytes as usize,
                width: self.width,
                height: self.height,
            }),
            _ => Err(Error::WrongCategory("not a compressed format")),
        }
    }

    // Rows tightly packed at `width * bpp`, dropping any `bytes_per_row`
    // padding. Borrows the raw payload when already tight (no padding to
    // drop); otherwise copies each row's unpadded prefix into a fresh
    // buffer. Shares `checked_len`'s `Truncated` bounds checks, AND
    // `aspect_bpp`'s aspect-aware sizing, with `rows()`/`pixels()`, so a
    // short/malformed payload - or a real fetched stencil aspect, sized at
    // its own 1 B/px rather than the format's nominal X-padded stride -
    // errors/packs the same way here as it does there.
    fn packed_bytes(&self) -> Result<Cow<'_, [u8]>, Error> {
        let bpp = self.aspect_bpp()?;
        let need = self.checked_len(bpp)?;
        let row_len = self.width as usize * bpp;
        let bpr = self.bytes_per_row as usize;
        if bpr == row_len {
            return Ok(Cow::Borrowed(&self.raw_bytes()[..need]));
        }
        // `checked_len` above already guarantees `bpr >= row_len` and
        // `raw_bytes().len() >= height * bpr`, so every row's
        // `start..start + row_len` slice below is in-bounds.
        let bytes = self.raw_bytes();
        let h = self.height as usize;
        let mut out = Vec::with_capacity(row_len * h);
        for y in 0..h {
            let start = y * bpr;
            out.extend_from_slice(&bytes[start..start + row_len]);
        }
        Ok(Cow::Owned(out))
    }

    // The format's per-pixel element size for flat typed/packed access
    // (Color, or a single-aspect depth/stencil); shared by `checked_bpp::<P>`
    // (which additionally checks it against `size_of::<P>()`) and
    // `packed_bytes` (which has no `P` to check against).
    //
    // For a single-aspect DepthStencil format the element size is the
    // present aspect's OWN size (`StencilKind::Uint8` -> 1,
    // `DepthKind::Float32` -> 4, `DepthKind::Unorm16` -> 2), NOT
    // `DepthStencilFormat::bytes_per_pixel`/`FormatKind::bytes_per_pixel()`.
    // That field includes X-padding baked into the nominal Metal format
    // (e.g. `X32_Stencil8`'s stencil aspect is fetched as 1 byte/pixel even
    // though the format's nominal stride is 8) - using the padded stride
    // here would make `pixels::<u8>`/`packed_bytes` on a real fetched
    // stencil texture falsely report `FormatMismatch`/`Truncated`.
    fn aspect_bpp(&self) -> Result<usize, Error> {
        match self.format_kind() {
            FormatKind::Color(c) => Ok(c.bytes_per_pixel),
            FormatKind::DepthStencil(d) => match (d.depth, d.stencil) {
                (Some(depth), None) => Ok(match depth {
                    crate::format::DepthKind::Unorm16 => 2,
                    crate::format::DepthKind::Float32 => 4,
                }),
                (None, Some(stencil)) => Ok(match stencil {
                    crate::format::StencilKind::Uint8 => 1,
                }),
                _ => Err(Error::WrongCategory(
                    "combined depth+stencil is fetched as separate aspects",
                )),
            },
            FormatKind::Compressed(_) => Err(Error::WrongCategory("compressed: use blocks()")),
            FormatKind::Unknown => Err(Error::UnknownFormat(self.pixel_format)),
        }
    }

    // `size_of::<P>()` must equal `aspect_bpp()`'s element size. Returns
    // that element size on success.
    fn checked_bpp<P: Pod>(&self) -> Result<usize, Error> {
        let bpp = self.aspect_bpp()?;
        if std::mem::size_of::<P>() != bpp {
            return Err(Error::FormatMismatch {
                requested: std::mem::size_of::<P>(),
                actual: bpp,
            });
        }
        Ok(bpp)
    }
}

/// A fetched texture (one base 2D image: a mip level and array slice). Raw
/// bytes are always available; typed pixel access decodes on demand,
/// exactly or errors.
pub struct Texture {
    stream_ref: u64,
    depth: u32,
    bytes_per_image: u32,
    plane: u32,
    slice: u32,
    level: u32,
    image: Image,
}

impl Texture {
    /// Builds a `Texture` from a fetched `TextureRecord` and its payload.
    /// Called by `Capture::textures_with`.
    pub(crate) fn from_parts(r: &TextureRecord, payload: Payload) -> Self {
        Self {
            stream_ref: r.stream_ref as u64,
            depth: r.depth as u32,
            bytes_per_image: r.bytes_per_image,
            plane: r.plane,
            slice: r.slice,
            level: r.level,
            image: Image {
                width: r.width,
                height: r.height as u32,
                bytes_per_row: r.bytes_per_row,
                pixel_format: r.pixel_format,
                payload,
            },
        }
    }

    /// Builds a `Texture` directly from parts, for unit testing without a
    /// live session.
    #[cfg(test)]
    pub(crate) fn for_test(
        stream_ref: u64,
        width: u32,
        height: u16,
        bpr: u32,
        fmt: u32,
        payload: Payload,
    ) -> Self {
        Self {
            stream_ref,
            depth: 1,
            bytes_per_image: 0,
            plane: 0,
            slice: 0,
            level: 0,
            image: Image {
                width,
                height: height as u32,
                bytes_per_row: bpr,
                pixel_format: fmt,
                payload,
            },
        }
    }

    /// The resource streamRef.
    pub fn stream_ref(&self) -> u64 {
        self.stream_ref
    }
    /// Width in texels.
    pub fn width(&self) -> u32 {
        self.image.width
    }
    /// Height in texels.
    pub fn height(&self) -> u32 {
        self.image.height
    }
    /// Depth in slices, straight from the fetched record. NOT reliable for
    /// detecting a 3D (volume) source texture: `GTReplayFetchTexture` has no
    /// z-plane selector and always serves exactly one fixed z-plane of a 3D
    /// texture (MEASURED, `known-3d.gputrace`, docs/findings/00-texture-fetch.md
    /// "3D volumes"), so this reads 1 even for a 16x16x4 volume's fetch.
    pub fn depth(&self) -> u32 {
        self.depth
    }
    /// Row stride in bytes (may exceed `width * bytes_per_pixel`).
    pub fn bytes_per_row(&self) -> u32 {
        self.image.bytes_per_row
    }
    /// Bytes per 2D image (one slice/level's worth of `bytes_per_row *
    /// height`, from the fetched record).
    pub fn bytes_per_image(&self) -> u32 {
        self.bytes_per_image
    }
    /// The texture plane this texture answers (0 for non-planar; see
    /// `Capture::texture_aspects` for combined depth/stencil). Always 0 for
    /// a natural `Capture::textures` fetch.
    pub fn plane(&self) -> u32 {
        self.plane
    }
    /// The array slice (or cube face) this texture answers. Always 0 for a
    /// natural `Capture::textures` fetch.
    pub fn slice(&self) -> u32 {
        self.slice
    }
    /// The mip level this texture answers. Always 0 for a natural
    /// `Capture::textures` fetch.
    pub fn level(&self) -> u32 {
        self.level
    }
    /// The canonical `MTLPixelFormat`.
    pub fn format(&self) -> MTLPixelFormat {
        self.image.format()
    }
    /// The decomposed format metadata.
    pub fn format_kind(&self) -> FormatKind {
        self.image.format_kind()
    }
    /// The raw payload bytes. Always available.
    pub fn raw_bytes(&self) -> &[u8] {
        self.image.raw_bytes()
    }

    /// The pixels as one contiguous slice of `P`. `Ok` only when the format
    /// is a byte-aligned `Color` (or single-aspect depth/stencil) whose
    /// element size equals `size_of::<P>()`, the rows are unpadded, AND the
    /// payload's sub-slice offset is a multiple of `align_of::<P>()`
    /// (`Err(Error::Misaligned)` otherwise - a non-element-aligned offset
    /// errors, it never panics); else `Err`.
    pub fn pixels<P: Pod>(&self) -> Result<&[P], Error> {
        self.image.pixels::<P>()
    }

    /// The pixels row by row, honouring `bytes_per_row` padding. Each item
    /// is one row's `width` pixels of `P`. `Err(Error::Misaligned)` if any
    /// row's start offset is not a multiple of `align_of::<P>()`.
    pub fn rows<P: Pod>(&self) -> Result<impl Iterator<Item = &[P]>, Error> {
        self.image.rows::<P>()
    }

    /// Raw compressed blocks + geometry, for a `Compressed` format.
    pub fn blocks(&self) -> Result<Blocks<'_>, Error> {
        self.image.blocks()
    }

    /// The pixel bytes with each row tightly packed at `width *
    /// bytes_per_pixel`, dropping any `bytes_per_row` padding. `Cow::Borrowed`
    /// when the rows are already tight (`bytes_per_row == width *
    /// bytes_per_pixel`); otherwise a row-by-row `Cow::Owned` copy. Shares
    /// `rows()`/`pixels()`'s `Truncated` bounds checks AND their aspect-aware
    /// sizing, so a short or malformed payload errors the same way, and a
    /// single-aspect depth/stencil format packs at that aspect's own element
    /// size (e.g. a fetched stencil aspect at 1 B/px, not the format's
    /// nominal X-padded stride). `Err(Error::WrongCategory)` for a
    /// compressed format (no per-pixel byte size to pack by; use `blocks()`
    /// there) or a combined depth+stencil format (fetched as separate
    /// aspects), `Err(Error::UnknownFormat)` for an unrecognized one.
    pub fn packed_bytes(&self) -> Result<Cow<'_, [u8]>, Error> {
        self.image.packed_bytes()
    }

    /// Builds a `Texture` directly from provenance parts, for unit testing
    /// `depth`/`bytes_per_image`/`plane`/`slice`/`level` without a live
    /// session. Geometry is a fixed 1x1 RGBA8Unorm, irrelevant to what this
    /// exercises.
    #[cfg(test)]
    pub(crate) fn for_test_provenance(
        depth: u32,
        bytes_per_image: u32,
        plane: u32,
        slice: u32,
        level: u32,
        payload: Payload,
    ) -> Self {
        Self {
            stream_ref: 0,
            depth,
            bytes_per_image,
            plane,
            slice,
            level,
            image: Image {
                width: 1,
                height: 1,
                bytes_per_row: 4,
                pixel_format: 70, // RGBA8Unorm
                payload,
            },
        }
    }
}

/// A fetched wireframe rendering (an offscreen debug-visualization image
/// keyed by dispatch UID, not a resource streamRef). Shares `Texture`'s
/// pixel-decoding logic.
pub struct Wireframe {
    dispatch_uid: u32,
    image: Image,
}

impl Wireframe {
    /// Builds a `Wireframe` from a fetched `WireframeRecord` and its
    /// payload. Called by `Capture::wireframes_with`.
    pub(crate) fn from_parts(r: &WireframeRecord, payload: Payload) -> Self {
        // `WireframeRecord` carries no `bytes_per_row` field (unlike
        // `TextureRecord`): wireframe renders are unpadded row-major images
        // - VERIFIED against a live fixture (`live_draws.rs`,
        // `fetch_wireframes_ground_truth`): a 256x256 R8Unorm wireframe's
        // payload is exactly `256 * 256 * 1` bytes, i.e. `bytes_per_row ==
        // width * bytes_per_pixel` with zero padding. Unknown/compressed
        // formats (never actually produced by wireframe fetch) fall back to
        // 0, which correctly fails typed access rather than guessing.
        //
        // This derivation assumes a `Color` format: `bytes_per_pixel()`
        // returns a `DepthStencil` format's nominal (X-padded) stride, the
        // exact value the aspect-size ruling in `checked_bpp` exists to
        // avoid. Wireframe renders are color images today (R8 measured); if
        // a depth/stencil wireframe path is ever added, this must be
        // revisited to use the aspect's own element size instead.
        let bpp = format_kind(r.pixel_format).bytes_per_pixel().unwrap_or(0) as u32;
        Self {
            dispatch_uid: r.dispatch_uid,
            image: Image {
                width: r.width,
                height: r.height as u32,
                bytes_per_row: r.width * bpp,
                pixel_format: r.pixel_format,
                payload,
            },
        }
    }

    /// Builds a `Wireframe` directly from parts, for unit testing without a
    /// live session.
    #[cfg(test)]
    pub(crate) fn for_test(width: u32, height: u16, bpr: u32, fmt: u32, payload: Payload) -> Self {
        Self {
            dispatch_uid: 0,
            image: Image {
                width,
                height: height as u32,
                bytes_per_row: bpr,
                pixel_format: fmt,
                payload,
            },
        }
    }

    /// The dispatchUID this wireframe render answers.
    pub fn dispatch_uid(&self) -> u32 {
        self.dispatch_uid
    }
    /// Width in texels.
    pub fn width(&self) -> u32 {
        self.image.width
    }
    /// Height in texels.
    pub fn height(&self) -> u32 {
        self.image.height
    }
    /// Row stride in bytes (may exceed `width * bytes_per_pixel`).
    pub fn bytes_per_row(&self) -> u32 {
        self.image.bytes_per_row
    }
    /// The canonical `MTLPixelFormat`.
    pub fn format(&self) -> MTLPixelFormat {
        self.image.format()
    }
    /// The decomposed format metadata.
    pub fn format_kind(&self) -> FormatKind {
        self.image.format_kind()
    }
    /// The raw payload bytes. Always available.
    pub fn raw_bytes(&self) -> &[u8] {
        self.image.raw_bytes()
    }

    /// The pixels as one contiguous slice of `P`. See `Texture::pixels`,
    /// including the `Err(Error::Misaligned)` case.
    pub fn pixels<P: Pod>(&self) -> Result<&[P], Error> {
        self.image.pixels::<P>()
    }

    /// The pixels row by row, honouring `bytes_per_row` padding. See
    /// `Texture::rows`.
    pub fn rows<P: Pod>(&self) -> Result<impl Iterator<Item = &[P]>, Error> {
        self.image.rows::<P>()
    }
}

/// Raw compressed blocks of a texture, with block geometry.
pub struct Blocks<'a> {
    /// The raw compressed block bytes.
    pub bytes: &'a [u8],
    /// Texels per block.
    pub block: (u8, u8),
    /// Bytes per block.
    pub block_bytes: u8,
    /// Blocks per row (from `bytes_per_row`); may be padded beyond what
    /// `width` strictly needs, see `expected_len`.
    pub blocks_per_row: usize,
    // Texture width/height in texels, kept only to compute `expected_len`'s
    // ceil-based block count (not part of the public per-block geometry
    // above, which is all `bytes_per_row`-derived).
    width: u32,
    height: u32,
}

impl Blocks<'_> {
    /// The tight compressed block-data length implied by the texture's
    /// actual geometry: `ceil(width / block.0) * ceil(height / block.1) *
    /// block_bytes`. This can be LESS than `blocks_per_row * <block rows> *
    /// block_bytes` when `bytes_per_row` pads `blocks_per_row` beyond what
    /// `width` strictly requires (the padding-inclusive stride `rows()` and
    /// `pixels()`-style checks reason about).
    ///
    /// `width`/`height` are wire data, not trustworthy alone: saturates
    /// (rather than panicking on overflow, as this file's other geometry
    /// arithmetic - `checked_len`, `packed_bytes` - also never panics on a
    /// malformed record) to `usize::MAX` for a pathological near-`u32::MAX`
    /// texture.
    pub fn expected_len(&self) -> usize {
        let cols = (self.width as usize).div_ceil(self.block.0 as usize);
        let rows = (self.height as usize).div_ceil(self.block.1 as usize);
        cols.saturating_mul(rows)
            .saturating_mul(self.block_bytes as usize)
    }

    /// The compressed block bytes tightly packed at `ceil(width / block.0)`
    /// blocks per row, dropping any padding blocks `bytes_per_row` added
    /// beyond that (mirrors `expected_len`'s row count). `Cow::Borrowed` when
    /// the fetched stride (`blocks_per_row * block_bytes`) is already tight;
    /// otherwise a row-by-row `Cow::Owned` copy. Symmetric with
    /// `Texture::packed_bytes`, including its failure mode:
    /// `Err(Error::Truncated)` when the fetched stride is narrower than the
    /// tight row width (the stride can't actually supply `cols` whole
    /// blocks per row - copying from it as if it could would silently
    /// overlap/duplicate block data across rows) or when `bytes` is shorter
    /// than the fetched stride implies. Length is `expected_len()` on
    /// success.
    pub fn packed_blocks(&self) -> Result<Cow<'_, [u8]>, Error> {
        let block_bytes = self.block_bytes as usize;
        let cols = (self.width as usize).div_ceil(self.block.0 as usize);
        let rows = (self.height as usize).div_ceil(self.block.1 as usize);
        let tight_row = cols.saturating_mul(block_bytes);
        let fetched_row = self.blocks_per_row.saturating_mul(block_bytes);

        // The fetched stride must supply at least a tight row's worth of
        // bytes, or copying `tight_row` bytes starting at each `y *
        // fetched_row` would run past the row's own data and into the
        // next row's (same failure `checked_len` guards against for
        // `bytes_per_row < row_len` in `packed_bytes`).
        if fetched_row < tight_row {
            return Err(Error::Truncated);
        }
        let need = rows.checked_mul(fetched_row).ok_or(Error::Truncated)?;
        if self.bytes.len() < need {
            return Err(Error::Truncated);
        }

        if fetched_row == tight_row {
            let end = self.expected_len().min(self.bytes.len());
            return Ok(Cow::Borrowed(&self.bytes[..end]));
        }

        // `need` above already guarantees `bytes.len() >= rows *
        // fetched_row`, and `tight_row <= fetched_row`, so every row's
        // `start..start + tight_row` slice below is in-bounds.
        let mut out = Vec::with_capacity(tight_row.saturating_mul(rows));
        for y in 0..rows {
            let start = y * fetched_row;
            out.extend_from_slice(&self.bytes[start..start + tight_row]);
        }
        Ok(Cow::Owned(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytes::{AlignedBuf, Payload};
    use std::sync::Arc;

    // Build a Texture directly from parts for unit testing (no live session).
    fn tex(width: u32, height: u16, bpr: u32, fmt: u32, bytes: &[u8]) -> Texture {
        let buf = Arc::new(AlignedBuf::from_bytes(bytes));
        Texture::for_test(
            0,
            width,
            height,
            bpr,
            fmt,
            Payload::new(buf, 0, bytes.len()),
        )
    }

    #[test]
    fn rgba32float_reads_as_f32() {
        let px = [1.0f32, 2.0, 3.0, 4.0];
        let bytes: Vec<u8> = bytemuck::cast_slice(&px).to_vec(); // 16 bytes, one pixel
        let t = tex(1, 1, 16, 125, &bytes); // RGBA32Float
        assert_eq!(t.pixels::<[f32; 4]>().unwrap(), &[[1.0, 2.0, 3.0, 4.0]]);
        // Wrong element size -> FormatMismatch, never a lossy read.
        assert!(matches!(
            t.pixels::<[u8; 4]>(),
            Err(crate::Error::FormatMismatch { .. })
        ));
        // Raw always available.
        assert_eq!(t.raw_bytes(), bytes.as_slice());
    }

    #[test]
    fn padded_rows_use_rows_not_pixels() {
        // 2x2 BGRA (bpp 4) with bpr 12 (4 bytes padding per row).
        let mut bytes = vec![0u8; 12 * 2];
        for y in 0..2 {
            for x in 0..2 {
                let i = y * 12 + x * 4;
                bytes[i..i + 4].copy_from_slice(&[x as u8, y as u8, 9, 9]);
            }
        }
        let t = tex(2, 2, 12, 80, &bytes); // BGRA8Unorm
        assert!(matches!(
            t.pixels::<[u8; 4]>(),
            Err(crate::Error::Padded { .. })
        ));
        let rows: Vec<Vec<[u8; 4]>> = t.rows::<[u8; 4]>().unwrap().map(|r| r.to_vec()).collect();
        assert_eq!(rows[1][0], [0, 1, 9, 9]);
    }

    #[test]
    fn depth32float_reads_as_f32() {
        let bytes: Vec<u8> = bytemuck::cast_slice(&[0.5f32; 4]).to_vec();
        let t = tex(2, 2, 8, 252, &bytes); // Depth32Float
        assert_eq!(t.pixels::<f32>().unwrap(), &[0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn x32_stencil8_reads_as_u8_not_padded_stride() {
        // MEASURED: the replayer serves a stencil aspect (X24/X32_Stencil8)
        // as 1 byte/pixel even though the nominal MTLPixelFormat stride
        // (`DepthStencilFormat::bytes_per_pixel`) is 8 for X32_Stencil8.
        // `pixels::<u8>()` must key on the aspect's own element size (1),
        // not the padded nominal stride, or this falsely reports
        // FormatMismatch (1 != 8).
        let bytes: Vec<u8> = vec![1, 2, 3, 4]; // 2x2, 1 byte/pixel
        let t = tex(2, 2, 2, 261, &bytes); // X32_Stencil8
        assert_eq!(t.pixels::<u8>().unwrap(), &[1, 2, 3, 4]);
    }

    #[test]
    fn wireframe_r8_reads_as_u8() {
        let bytes: Vec<u8> = vec![10, 20, 30, 40];
        let buf = Arc::new(AlignedBuf::from_bytes(&bytes));
        let w = Wireframe::for_test(4, 1, 4, 10, Payload::new(buf, 0, bytes.len())); // R8Unorm
        assert_eq!(w.pixels::<u8>().unwrap(), &[10, 20, 30, 40]);
        assert_eq!(w.dispatch_uid(), 0);
    }

    #[test]
    fn depth_bytes_per_image_and_provenance_are_carried_from_the_record() {
        let bytes: Vec<u8> = vec![0; 4];
        let buf = Arc::new(AlignedBuf::from_bytes(&bytes));
        let t = Texture::for_test_provenance(4, 1024, 1, 2, 3, Payload::new(buf, 0, 4));
        assert_eq!(t.depth(), 4);
        assert_eq!(t.bytes_per_image(), 1024);
        assert_eq!(t.plane(), 1);
        assert_eq!(t.slice(), 2);
        assert_eq!(t.level(), 3);
    }

    #[test]
    fn truncated_payload_errors_not_panics_slop_case() {
        // 1x1 RGBA8Unorm (bpp 4, bpr 4) but only 3 bytes: too short even for
        // one whole pixel. Must error before `bytemuck::cast_slice` ever
        // sees it (that would panic with OutputSliceWouldHaveSlop).
        let bytes: Vec<u8> = vec![1, 2, 3];
        let t = tex(1, 1, 4, 70, &bytes); // RGBA8Unorm
        assert!(matches!(
            t.pixels::<[u8; 4]>(),
            Err(crate::Error::Truncated)
        ));
    }

    #[test]
    fn truncated_payload_errors_not_panics_short_read_case() {
        // 4x4 RGBA8Unorm (bpr 16, needs 64 bytes) given only 8: a
        // size-divisible shortfall that `bytemuck::cast_slice` would have
        // silently accepted as 2 pixels instead of the declared 16.
        let bytes: Vec<u8> = vec![0; 8];
        let t = tex(4, 4, 16, 70, &bytes); // RGBA8Unorm
        assert!(matches!(
            t.pixels::<[u8; 4]>(),
            Err(crate::Error::Truncated)
        ));
    }

    #[test]
    fn truncated_payload_errors_from_rows() {
        // 2x4 R8Unorm with bpr 2 (needs 8 bytes) but only 4 bytes: the
        // second row's slice would run past the end of the payload.
        let bytes: Vec<u8> = vec![0; 4];
        let t = tex(2, 4, 2, 10, &bytes); // R8Unorm
        assert!(matches!(t.rows::<u8>(), Err(crate::Error::Truncated)));
    }

    #[test]
    fn overlong_payload_pixels_is_not_inflated() {
        // 1x1 RGBA8Unorm (bpr 4, needs 4 bytes) given 8 bytes: the excess is
        // a whole extra `size_of::<[u8; 4]>()`-sized pixel's worth. Casting
        // the whole payload (instead of just the geometry-implied region)
        // would silently return 2 pixels instead of the declared 1.
        let bytes: Vec<u8> = vec![1, 2, 3, 4, 9, 9, 9, 9];
        let t = tex(1, 1, 4, 70, &bytes); // RGBA8Unorm
        let px = t.pixels::<[u8; 4]>().unwrap();
        assert_eq!(px.len(), 1);
        assert_eq!(px, &[[1, 2, 3, 4]]);
    }

    #[test]
    fn bytes_per_row_shorter_than_row_errors_not_panics_in_rows() {
        // 2x2 RGBA8Unorm (bpp 4): bytes_per_row (4) is shorter than one
        // whole row (width * bpp = 8), but the payload is exactly `height *
        // bytes_per_row` (8) bytes long - long enough to satisfy the old
        // `checked_len` guard alone. Without also requiring `bytes_per_row
        // >= width * bpp`, `rows()`'s second-row slice (`4..12`) would run
        // past the 8-byte payload and panic instead of erroring.
        let bytes: Vec<u8> = vec![0; 8];
        let t = tex(2, 2, 4, 70, &bytes); // RGBA8Unorm, bpr=4 < width*bpp=8
        assert!(matches!(t.rows::<[u8; 4]>(), Err(crate::Error::Truncated)));
    }

    #[test]
    fn bytes_per_row_shorter_than_row_errors_not_panics_in_pixels() {
        // Same shape as above, through `pixels()`: bpr (4) != row_len (8),
        // so this already errors via the `Padded` check before `checked_len`
        // is even reached - confirming the fix doesn't disturb that path.
        let bytes: Vec<u8> = vec![0; 8];
        let t = tex(2, 2, 4, 70, &bytes);
        assert!(t.pixels::<[u8; 4]>().is_err());
    }

    #[test]
    fn packed_bytes_borrows_when_already_tight() {
        // 2x2 RGBA8Unorm (bpp 4), bpr == width*bpp: no padding to drop.
        let bytes: Vec<u8> = (0..16).collect();
        let t = tex(2, 2, 8, 70, &bytes); // RGBA8Unorm
        let packed = t.packed_bytes().unwrap();
        assert!(matches!(packed, Cow::Borrowed(_)));
        assert_eq!(&*packed, bytes.as_slice());
    }

    #[test]
    fn packed_bytes_copies_and_drops_padding_when_padded() {
        // 2x2 BGRA (bpp 4) with bpr 12 (4 bytes padding per row), same shape
        // as `padded_rows_use_rows_not_pixels`.
        let mut bytes = vec![0u8; 12 * 2];
        for y in 0..2 {
            for x in 0..2 {
                let i = y * 12 + x * 4;
                bytes[i..i + 4].copy_from_slice(&[x as u8, y as u8, 9, 9]);
            }
        }
        let t = tex(2, 2, 12, 80, &bytes); // BGRA8Unorm
        let packed = t.packed_bytes().unwrap();
        assert!(matches!(packed, Cow::Owned(_)));
        // Tight length: height * width * bpp, no padding.
        assert_eq!(packed.len(), 2 * 2 * 4);
        // Row 0's pixels, then row 1's pixels immediately after (the
        // padding bytes at bytes[8..12] of the original buffer are gone).
        assert_eq!(&packed[0..4], &[0, 0, 9, 9]);
        assert_eq!(&packed[4..8], &[1, 0, 9, 9]);
        assert_eq!(&packed[8..12], &[0, 1, 9, 9]);
        assert_eq!(&packed[12..16], &[1, 1, 9, 9]);
    }

    #[test]
    fn packed_bytes_errors_wrong_category_for_compressed() {
        let bytes = vec![0u8; 16];
        let t = tex(4, 4, 16, 204, &bytes); // ASTC_4x4_LDR
        assert!(matches!(
            t.packed_bytes(),
            Err(crate::Error::WrongCategory(_))
        ));
    }

    #[test]
    fn packed_bytes_stencil_aspect_sizes_at_1_byte_not_padded_stride() {
        // MEASURED (see `x32_stencil8_reads_as_u8_not_padded_stride`): a
        // real fetched X32_Stencil8 stencil aspect is served 1 B/px, even
        // though the format's nominal stride (`FormatKind::bytes_per_pixel`)
        // is 8. `packed_bytes` must size by the aspect's own 1 B/px (via
        // `aspect_bpp`, shared with `pixels`/`rows`), or it would compute
        // `row_len = width*8` against a `bytes_per_row == width` payload and
        // falsely report `Truncated` instead of packing successfully.
        let bytes: Vec<u8> = vec![1, 2, 3, 4]; // 2x2, 1 byte/pixel, already tight
        let t = tex(2, 2, 2, 261, &bytes); // X32_Stencil8, bpr == width
        let packed = t.packed_bytes().unwrap();
        assert!(matches!(packed, Cow::Borrowed(_)));
        assert_eq!(packed.len(), 2 * 2);
        assert_eq!(&*packed, bytes.as_slice());
    }

    #[test]
    fn blocks_expected_len_is_ceil_based_not_padded() {
        // ASTC_4x4_LDR (fmt 204): block (4,4), block_bytes 16. A 6x5 texel
        // texture needs ceil(6/4)=2 x ceil(5/4)=2 = 4 blocks tight (64 B),
        // even though bytes_per_row here is padded to 3 blocks/row (48 B).
        let bytes = vec![0u8; 48 * 2];
        let t = tex(6, 5, 48, 204, &bytes);
        let blocks = t.blocks().unwrap();
        assert_eq!(blocks.blocks_per_row, 3);
        assert_eq!(blocks.expected_len(), 64);
        assert!(blocks.expected_len() < blocks.blocks_per_row * 2 * blocks.block_bytes as usize);
    }

    #[test]
    fn packed_blocks_borrows_when_already_tight() {
        // ASTC_4x4_LDR (fmt 204), 8x8 texels: cols=rows=2 blocks, block_bytes
        // 16, blocks_per_row=2 (bpr=32) - no padding blocks to drop.
        let bytes: Vec<u8> = (0..64).collect();
        let t = tex(8, 8, 32, 204, &bytes);
        let blocks = t.blocks().unwrap();
        let packed = blocks.packed_blocks().unwrap();
        assert!(matches!(packed, Cow::Borrowed(_)));
        assert_eq!(&*packed, &bytes[..blocks.expected_len()]);
    }

    #[test]
    fn packed_blocks_copies_and_drops_padding_when_padded() {
        // ASTC_4x4_LDR (fmt 204), 6x5 texels: cols=ceil(6/4)=2 tight, but
        // bytes_per_row is padded to 3 blocks/row (48 B); rows=ceil(5/4)=2.
        // A marker byte at each block's start lets us confirm the dropped
        // (3rd) column and the packed row boundary.
        let bpr = 48; // 3 blocks/row * 16 B/block
        let mut bytes = vec![0u8; bpr * 2];
        for row in 0..2usize {
            for col in 0..3usize {
                bytes[row * bpr + col * 16] = (row * 3 + col) as u8;
            }
        }
        let t = tex(6, 5, 48, 204, &bytes);
        let blocks = t.blocks().unwrap();
        let packed = blocks.packed_blocks().unwrap();
        assert!(matches!(packed, Cow::Owned(_)));
        assert_eq!(packed.len(), blocks.expected_len()); // 2*2*16 = 64
        // Row 0: col0, col1 kept (col2's padding block dropped).
        assert_eq!(packed[0], 0);
        assert_eq!(packed[16], 1);
        // Row 1 starts immediately after row 0's tight 32 B.
        assert_eq!(packed[32], 3);
        assert_eq!(packed[48], 4);
    }

    #[test]
    fn packed_blocks_errors_truncated_on_a_narrow_fetched_stride() {
        // ASTC_4x4_LDR (fmt 204), 8x8 texels: cols=rows=2 blocks (tight_row =
        // 2*16 = 32 B), but bytes_per_row here is padded down to only 1
        // block/row (bpr=16) - narrower than what `width` needs. Copying
        // `tight_row` bytes starting at each `y * fetched_row` would run
        // past each row's own 16 B and duplicate the next row's data, so
        // this must error instead of silently corrupting.
        let bytes = vec![0u8; 16 * 2];
        let t = tex(8, 8, 16, 204, &bytes);
        let blocks = t.blocks().unwrap();
        assert_eq!(blocks.blocks_per_row, 1);
        assert!(matches!(
            blocks.packed_blocks(),
            Err(crate::Error::Truncated)
        ));
    }

    #[test]
    fn misaligned_offset_errors_not_panics_pixels() {
        // Depth32Float (element f32, align 4), 1x1, bpr=4. Payload offset 1
        // into a 16-byte-aligned buffer: the pixel region then starts at an
        // address that is NOT a multiple of 4, so the zero-copy `&[u8] ->
        // &[f32]` cast must error (`Misaligned`), never panic.
        let src: Vec<u8> = vec![0u8; 5]; // 1 byte pad + 4-byte f32 pixel
        let buf = Arc::new(AlignedBuf::from_bytes(&src));
        let payload = Payload::new(buf, 1, 4);
        let t = Texture::for_test(0, 1, 1, 4, 252, payload); // Depth32Float
        assert!(matches!(
            t.pixels::<f32>(),
            Err(crate::Error::Misaligned { align: 4 })
        ));
    }
}
