//! Parser for the harvester "capture" block format (dossier 02). Pure safe
//! Rust: no FFI, no session. A capture-bundle `MTLTexture-*` file IS such a
//! block, so these can be read standalone.
use crate::HarvesterError;
use crate::util::{u16_at, u32_at, u64_at};

const MAGIC: [u8; 8] = [0x65, 0x72, 0x75, 0x74, 0x70, 0x61, 0x63, 0x00];

/// Byte offsets and sizes within a "capture" block (dossier 02). The header
/// runs `[0x00, HEADER_LEN)`; plane descriptors are packed from `HEADER_LEN`,
/// `PLANE_DESC_LEN` bytes each.
const TYPE_OFF: usize = 0x0a; // u16 type tag (1 = texture)
const METADATA_SIZE_OFF: usize = 0x0c; // u32 metadata size
const PLANE_COUNT_OFF: usize = 0x10; // u64 plane count
const HEADER_LEN: usize = 0x18; // header size == offset of plane[0]
const PLANE_DESC_LEN: usize = 0x30; // one plane descriptor, six u64 fields

/// One texture plane's descriptor (six `u64` fields).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaneDescriptor {
    /// `MTLPixelFormat` (e.g. 80 = BGRA8Unorm).
    pub pixel_format: u32,
    /// Width in texels.
    pub width: u64,
    /// Height in texels.
    pub height: u64,
    /// Depth / slice count.
    pub depth: u64,
    /// Bytes per row.
    pub bytes_per_row: u64,
    /// Bytes per image (= plane size).
    pub bytes_per_image: u64,
}

/// A validated "capture" metadata block over a borrowed byte buffer.
#[derive(Debug, Clone, Copy)]
pub struct CaptureBlock<'a> {
    bytes: &'a [u8],
    metadata_size: usize,
    plane_count: u64,
}

impl<'a> CaptureBlock<'a> {
    /// Validate `bytes` as a texture capture block.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, HarvesterError> {
        if bytes.len() < HEADER_LEN || bytes[..8] != MAGIC {
            return Err(HarvesterError::BadMagic);
        }
        let ty = u16_at(bytes, TYPE_OFF);
        if ty != 1 {
            return Err(HarvesterError::WrongType(ty));
        }
        let metadata_size = u32_at(bytes, METADATA_SIZE_OFF) as usize;
        let plane_count = u64_at(bytes, PLANE_COUNT_OFF);
        // Every plane descriptor and the metadata region must fit.
        let planes_end = (plane_count as usize)
            .checked_mul(PLANE_DESC_LEN)
            .and_then(|x| x.checked_add(HEADER_LEN))
            .ok_or(HarvesterError::Truncated)?;
        if metadata_size > bytes.len() || planes_end > bytes.len() {
            return Err(HarvesterError::Truncated);
        }
        Ok(Self {
            bytes,
            metadata_size,
            plane_count,
        })
    }

    /// Number of texture planes.
    pub fn plane_count(&self) -> u64 {
        self.plane_count
    }

    /// The `i`-th plane descriptor, or `None` if `i >= plane_count`.
    pub fn plane(&self, i: u64) -> Option<PlaneDescriptor> {
        if i >= self.plane_count {
            return None;
        }
        let o = (i as usize)
            .checked_mul(PLANE_DESC_LEN)
            .and_then(|x| x.checked_add(HEADER_LEN))?;
        // Ensure the descriptor (PLANE_DESC_LEN bytes) fits in the buffer.
        if o.checked_add(PLANE_DESC_LEN)
            .is_none_or(|end| end > self.bytes.len())
        {
            return None;
        }
        Some(PlaneDescriptor {
            pixel_format: u64_at(self.bytes, o) as u32,
            width: u64_at(self.bytes, o + 0x08),
            height: u64_at(self.bytes, o + 0x10),
            depth: u64_at(self.bytes, o + 0x18),
            bytes_per_row: u64_at(self.bytes, o + 0x20),
            bytes_per_image: u64_at(self.bytes, o + 0x28),
        })
    }

    /// The data payload after the metadata.
    pub fn data(&self) -> &'a [u8] {
        &self.bytes[self.metadata_size..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn synthetic() -> Vec<u8> {
        let mut b = vec![0u8; 0x78 + 4];
        b[..8].copy_from_slice(&[0x65, 0x72, 0x75, 0x74, 0x70, 0x61, 0x63, 0x00]);
        b[0x08..0x0a].copy_from_slice(&2u16.to_le_bytes()); // version
        b[0x0a..0x0c].copy_from_slice(&1u16.to_le_bytes()); // type = texture
        b[0x0c..0x10].copy_from_slice(&0x78u32.to_le_bytes()); // metadataSize
        b[0x10..0x18].copy_from_slice(&1u64.to_le_bytes()); // planeCount
        // plane 0 descriptor: fmt 80, 64x64x1, bpr 256, size 16384
        let desc = [80u64, 64, 64, 1, 256, 16384];
        for (i, v) in desc.iter().enumerate() {
            let o = 0x18 + i * 8;
            b[o..o + 8].copy_from_slice(&v.to_le_bytes());
        }
        b[0x78..].copy_from_slice(b"DATA");
        b
    }

    #[test]
    fn parses_a_capture_block() {
        let bytes = synthetic();
        let block = CaptureBlock::parse(&bytes).unwrap();
        assert_eq!(block.plane_count(), 1);
        let p = block.plane(0).unwrap();
        assert_eq!(p.pixel_format, 80);
        assert_eq!(p.width, 64);
        assert_eq!(p.height, 64);
        assert_eq!(p.bytes_per_row, 256);
        assert_eq!(p.bytes_per_image, 16384);
        assert_eq!(block.data(), b"DATA");
        assert!(block.plane(1).is_none());
    }

    #[test]
    fn rejects_bad_magic_and_truncation() {
        assert!(matches!(
            CaptureBlock::parse(&[0u8; 0x20]),
            Err(HarvesterError::BadMagic)
        ));
        let mut short = synthetic();
        short.truncate(0x10);
        assert!(matches!(
            CaptureBlock::parse(&short),
            Err(HarvesterError::BadMagic | HarvesterError::Truncated)
        ));
    }

    #[test]
    fn agrees_with_the_framework_getters() {
        use core::ffi::c_void;
        let bytes = synthetic();
        let block = bytes.as_ptr() as *const c_void;
        // SAFETY: `bytes` is a valid capture block of `bytes.len()`; the getters
        // validate the magic and only compute in-bounds offsets.
        unsafe {
            assert!(
                !gputools_replay_sys::ffi::GTHarvesterGetMetadata(block, bytes.len()).is_null()
            );
            assert_eq!(
                gputools_replay_sys::ffi::GTHarvesterGetTexturePlaneCount(block),
                1
            );
            let data = gputools_replay_sys::ffi::GTHarvesterGetData(block, bytes.len()) as usize;
            assert_eq!(data - bytes.as_ptr() as usize, 0x78);
        }
    }

    #[test]
    fn rejects_overflowing_plane_count_without_panicking() {
        let mut b = vec![0u8; 0x18];
        b[..8].copy_from_slice(&[0x65, 0x72, 0x75, 0x74, 0x70, 0x61, 0x63, 0x00]);
        b[0x0a..0x0c].copy_from_slice(&1u16.to_le_bytes()); // type = texture
        b[0x0c..0x10].copy_from_slice(&0x18u32.to_le_bytes()); // metadataSize
        b[0x10..0x18].copy_from_slice(&u64::MAX.to_le_bytes()); // planeCount = MAX
        assert!(matches!(
            CaptureBlock::parse(&b),
            Err(HarvesterError::Truncated)
        ));
    }
}
