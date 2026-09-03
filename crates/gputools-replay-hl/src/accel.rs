//! Fetched acceleration structures (dossier 05 geometry): raw bytes always
//! available, header fields and triangle vertex data decoded on demand.

use crate::Error;
use crate::bytes::Payload;

/// An axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// The minimum corner (x, y, z).
    pub min: [f32; 3],
    /// The maximum corner (x, y, z).
    pub max: [f32; 3],
}

/// A fetched acceleration structure. Raw bytes always; geometry on demand.
pub struct AccelStructure {
    payload: Payload,
}

impl AccelStructure {
    /// Builds an `AccelStructure` from a fetched `AccelRecord`'s payload.
    /// Called by `Capture::acceleration_structures`.
    pub(crate) fn from_parts(payload: Payload) -> Self {
        Self { payload }
    }

    /// The raw bytes. Always available.
    pub fn raw_bytes(&self) -> &[u8] {
        self.payload.bytes()
    }

    /// Total size field (`0x08`, u64) - the structure's declared byte size
    /// (dossier 05: `0x08 == 0x718 == 1816` for a one-triangle primitive
    /// structure, matching the info record's own `size` field). `None` if
    /// the payload is shorter than the field requires.
    pub fn total_size(&self) -> Option<u64> {
        read_u64(self.raw_bytes(), 0x08)
    }

    /// The geometry AABB (`0x0a0..0x0b4`, six f32 min/max), if the payload is
    /// long enough.
    pub fn aabb(&self) -> Option<Aabb> {
        let b = self.raw_bytes();
        Some(Aabb {
            min: [
                read_f32(b, 0x0a0)?,
                read_f32(b, 0x0a4)?,
                read_f32(b, 0x0a8)?,
            ],
            max: [
                read_f32(b, 0x0ac)?,
                read_f32(b, 0x0b0)?,
                read_f32(b, 0x0b4)?,
            ],
        })
    }

    /// The triangle vertices (`0x418..`, tightly-packed f32 triples), one
    /// `[f32; 3]` per vertex (three consecutive elements make one triangle) -
    /// bounded to exactly the header's triangle/primitive count field
    /// (`0x2c`, u32), i.e. `count * 3` vertices, never more.
    ///
    /// MEASURED against `captures/accel-structure.gputrace`'s one-triangle
    /// bottom-level structure: the `0x2c` field reads `1` there (dumped via
    /// `probes/run.sh rawfetch GTReplayFetchAccelerationStructure`), matching
    /// dossier 05's controlled-variation measurement (1, 2, 10 triangles ->
    /// the field reads 1, 2, 10). Validated only at that one confirmed
    /// value; a multi-triangle capture would exercise the `> 1` case for the
    /// first time. `raw_bytes()` remains authoritative regardless.
    ///
    /// This replaces v1's "read every tightly-packed `[f32; 3]` from `0x418`
    /// to end-of-payload" behaviour, which - for a 1-triangle, 1816-byte
    /// structure - reinterpreted ~61 unrelated trailing structure bytes
    /// (other, not-yet-decoded sections) as phantom vertices. Trusting the
    /// now-confirmed count field is preferred to that guess.
    ///
    /// Returns `Err(Error::Truncated)` if the payload is too short to hold
    /// the count field itself, or shorter than `0x418 + count * 3 *
    /// size_of::<f32>()` requires. Returns `Err(Error::Misaligned)` on the
    /// (unexpected, since `0x418` and the vertex stride are both 4-aligned)
    /// case of a non-4-byte-aligned sub-slice offset.
    pub fn triangles(&self) -> Result<&[[f32; 3]], Error> {
        let b = self.raw_bytes();
        let count = read_u32(b, TRIANGLE_COUNT_OFFSET).ok_or(Error::Truncated)? as usize;
        let need = count
            .checked_mul(3)
            .and_then(|verts| verts.checked_mul(std::mem::size_of::<[f32; 3]>()))
            .ok_or(Error::Truncated)?;
        let end = VERTICES_OFFSET.checked_add(need).ok_or(Error::Truncated)?;
        let region = b.get(VERTICES_OFFSET..end).ok_or(Error::Truncated)?;
        bytemuck::try_cast_slice(region).map_err(|_| Error::Misaligned {
            align: std::mem::align_of::<[f32; 3]>(),
        })
    }
}

/// `0x2c`, u32: the triangle/primitive count (dossier 05, MEASURED against
/// `accel-structure.gputrace` - see `triangles()`).
const TRIANGLE_COUNT_OFFSET: usize = 0x2c;
/// `0x418`: the start of the tightly-packed `[f32; 3]` vertex data.
const VERTICES_OFFSET: usize = 0x418;

/// Little-endian `u64` at `off`, or `None` if the payload is too short.
fn read_u64(b: &[u8], off: usize) -> Option<u64> {
    b.get(off..off + 8)?.try_into().ok().map(u64::from_le_bytes)
}

/// Little-endian `f32` at `off`, or `None` if the payload is too short.
fn read_f32(b: &[u8], off: usize) -> Option<f32> {
    b.get(off..off + 4)?.try_into().ok().map(f32::from_le_bytes)
}

/// Little-endian `u32` at `off`, or `None` if the payload is too short.
fn read_u32(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)?.try_into().ok().map(u32::from_le_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytes::AlignedBuf;
    use std::sync::Arc;

    // A synthetic 1816-byte block (dossier 05's ground-truthed one-triangle
    // primitive size): total_size at 0x08, the triangle count (1) at 0x2c,
    // an AABB at 0x0a0, and one triangle's three vertices at 0x418.
    // Everything else is zeroed.
    fn synthetic_block() -> Vec<u8> {
        let mut b = vec![0u8; 1816];
        b[0x08..0x10].copy_from_slice(&1816u64.to_le_bytes());
        b[0x2c..0x30].copy_from_slice(&1u32.to_le_bytes());
        let aabb: [f32; 6] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]; // min, then max
        b[0x0a0..0x0b8].copy_from_slice(bytemuck::cast_slice(&aabb));
        let triangle: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        b[0x418..0x418 + 36].copy_from_slice(bytemuck::cast_slice(&triangle));
        b
    }

    fn accel(bytes: &[u8]) -> AccelStructure {
        let buf = Arc::new(AlignedBuf::from_bytes(bytes));
        AccelStructure::from_parts(Payload::new(buf, 0, bytes.len()))
    }

    #[test]
    fn total_size_reads_the_header_field() {
        let a = accel(&synthetic_block());
        assert_eq!(a.total_size(), Some(1816));
    }

    #[test]
    fn aabb_reads_min_and_max() {
        let a = accel(&synthetic_block());
        assert_eq!(
            a.aabb(),
            Some(Aabb {
                min: [1.0, 2.0, 3.0],
                max: [4.0, 5.0, 6.0],
            })
        );
    }

    #[test]
    fn triangles_is_bounded_to_exactly_the_header_count() {
        let a = accel(&synthetic_block());
        let verts = a.triangles().unwrap();
        // count (0x2c) is 1 triangle = 3 vertices, NOT "read to end of
        // payload" (v1's behaviour, which would have returned 64 here: 1816
        // - 0x418 = 768 bytes = 64 whole [f32; 3] elements, 61 of them
        // phantom bytes from other structure sections).
        assert_eq!(verts.len(), 3);
        assert_eq!(verts, &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    }

    #[test]
    fn total_size_and_aabb_are_none_on_a_short_payload() {
        // 4 bytes: too short even for the `0x08` u64 field.
        let a = accel(&[0u8; 4]);
        assert_eq!(a.total_size(), None);
        assert_eq!(a.aabb(), None);
    }

    #[test]
    fn triangles_errors_truncated_on_a_short_payload() {
        // Too short even for the `0x2c` count field itself.
        let a = accel(&[0u8; 16]);
        assert!(matches!(a.triangles(), Err(Error::Truncated)));
    }

    #[test]
    fn triangles_errors_truncated_when_payload_too_short_for_declared_count() {
        // Header claims 2 triangles (0x2c = 2) but the payload ends right
        // after the first triangle's vertices (0x418 + 36): not enough
        // bytes for the second triangle the count promises.
        let mut b = vec![0u8; 0x418 + 36];
        b[0x2c..0x30].copy_from_slice(&2u32.to_le_bytes());
        let a = accel(&b);
        assert!(matches!(a.triangles(), Err(Error::Truncated)));
    }

    #[test]
    fn triangles_misaligned_offset_errors_not_panics() {
        // A 1-byte-offset Payload into a 16-byte-aligned buffer: 0x418 + 1
        // is not a multiple of align_of::<[f32; 3]>() (4), so the zero-copy
        // cast must error, not panic.
        let mut src = vec![0u8; 1 + 0x418 + 36];
        src[1 + 0x2c..1 + 0x30].copy_from_slice(&1u32.to_le_bytes());
        let triangle: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        src[1 + 0x418..1 + 0x418 + 36].copy_from_slice(bytemuck::cast_slice(&triangle));
        let buf = Arc::new(AlignedBuf::from_bytes(&src));
        let a = AccelStructure::from_parts(Payload::new(buf, 1, src.len() - 1));
        assert!(matches!(a.triangles(), Err(Error::Misaligned { align: 4 })));
    }
}
