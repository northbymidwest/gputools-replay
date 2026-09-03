//! A 16-byte-aligned, shared, immutable payload buffer. Every domain handle
//! slices a [`Payload`] rather than holding its own copy. The 16-byte
//! guarantee is on the *underlying buffer's start only*: a [`Payload`] is a
//! `buf[off..off + len]` sub-slice, and `off` (a wire-provided
//! `data_offset`, or a per-row stride computed from one) is not guaranteed
//! to itself be a multiple of 16, or of any particular element's alignment.
//! So a `bytemuck` zero-copy cast (`&[u8] -> &[f32]`, etc.) over a
//! `Payload`'s bytes can still fail on alignment - callers use
//! `bytemuck::try_cast_slice` and surface that as a crate `Error`
//! (`Error::Misaligned`), never `bytemuck::cast_slice`, which would panic.
//!
//! `AlignedBuf`/`Payload` are consumed by every domain handle's `from_parts`,
//! called from `capture.rs`'s `Capture` fetch methods.

use std::sync::Arc;

// The field is only ever read through `bytemuck::cast_slice`, which does not
// count as a read for dead-code analysis - the whole point of this type.
#[allow(dead_code)]
#[repr(align(16))]
#[derive(Clone, Copy)]
struct Align16([u8; 16]);

// SAFETY: `Align16` is `[u8; 16]` - every bit pattern is valid, there is no
// padding, and `repr(align(16))` only strengthens alignment (it does not
// affect validity).
unsafe impl bytemuck::Zeroable for Align16 {}
// SAFETY: `Align16` is `[u8; 16]` - every bit pattern is valid, there is no
// padding, and `repr(align(16))` only strengthens alignment (it does not
// affect validity).
unsafe impl bytemuck::Pod for Align16 {}

/// A 16-byte-aligned owned byte buffer (so `bytemuck` casts always align).
pub(crate) struct AlignedBuf {
    data: Box<[Align16]>,
    len: usize,
}

impl AlignedBuf {
    /// Copies `src` into a freshly allocated, 16-byte-aligned buffer.
    pub(crate) fn from_bytes(src: &[u8]) -> Self {
        let n = src.len().div_ceil(16).max(1);
        let mut data = vec![Align16([0; 16]); n].into_boxed_slice();
        bytemuck::cast_slice_mut::<Align16, u8>(&mut data)[..src.len()].copy_from_slice(src);
        AlignedBuf {
            data,
            len: src.len(),
        }
    }

    /// The buffer's bytes, exactly `src.len()` long, 16-byte aligned.
    pub(crate) fn as_slice(&self) -> &[u8] {
        &bytemuck::cast_slice::<Align16, u8>(&self.data)[..self.len]
    }
}

/// A shared, aligned, immutable slice of one reply's payload buffer.
#[derive(Clone)]
pub(crate) struct Payload {
    buf: Arc<AlignedBuf>,
    off: usize,
    len: usize,
}

impl Payload {
    /// A view onto `buf[off..off + len]`.
    pub(crate) fn new(buf: Arc<AlignedBuf>, off: usize, len: usize) -> Self {
        Self { buf, off, len }
    }

    /// The `off..off + len` sub-slice of the underlying aligned buffer.
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.buf.as_slice()[self.off..self.off + self.len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn payload_is_16_aligned_and_exact() {
        let src: Vec<u8> = (0..37).collect(); // odd length
        let buf = std::sync::Arc::new(AlignedBuf::from_bytes(&src));
        let p = Payload::new(buf, 4, 8); // sub-slice [4,12)
        assert_eq!(p.bytes(), &src[4..12]);
        assert_eq!(
            AlignedBuf::from_bytes(&src).as_slice().as_ptr() as usize % 16,
            0
        );
    }
}
