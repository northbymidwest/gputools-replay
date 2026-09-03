//! Fetched buffers and heaps: raw bytes always available, typed slice views
//! on demand via `bytemuck`.

use crate::Error;
use crate::bytes::Payload;
use bytemuck::Pod;

/// A fetched buffer. Raw bytes always; typed slice views on demand.
pub struct Buffer {
    stream_ref: u64,
    payload: Payload,
}

impl Buffer {
    /// Builds a `Buffer` from a fetched `BufferRecord`'s streamRef and
    /// payload. Called by `Capture::buffers`.
    pub(crate) fn from_parts(stream_ref: u64, payload: Payload) -> Self {
        Self {
            stream_ref,
            payload,
        }
    }

    /// The resource streamRef.
    pub fn stream_ref(&self) -> u64 {
        self.stream_ref
    }
    /// The raw bytes. Always available.
    pub fn raw_bytes(&self) -> &[u8] {
        self.payload.bytes()
    }
    /// A typed slice view. `Err(FormatMismatch)` if the byte length is not a
    /// whole number of `size_of::<T>()`, or on misalignment: the payload's
    /// `data_offset` is wire-provided and only as aligned as it happens to
    /// be, not guaranteed a multiple of `align_of::<T>()` (only the
    /// underlying buffer's own start is guaranteed 16-byte aligned; see
    /// `bytes.rs`) - `try_cast_slice` turns that into this `Err`, never a
    /// panic.
    pub fn as_slice<T: Pod>(&self) -> Result<&[T], Error> {
        bytemuck::try_cast_slice(self.raw_bytes()).map_err(|_| Error::FormatMismatch {
            requested: std::mem::size_of::<T>(),
            actual: self.raw_bytes().len(),
        })
    }
}

/// A fetched heap's backing store. Raw bytes always; typed slice views on
/// demand. Identical shape to [`Buffer`]: a heap's payload is simply its
/// full backing store, sub-allocated buffers included.
pub struct Heap {
    stream_ref: u64,
    payload: Payload,
}

impl Heap {
    /// Builds a `Heap` from a fetched `HeapRecord`'s streamRef and payload.
    /// Called by `Capture::heaps`.
    pub(crate) fn from_parts(stream_ref: u64, payload: Payload) -> Self {
        Self {
            stream_ref,
            payload,
        }
    }

    /// The resource streamRef.
    pub fn stream_ref(&self) -> u64 {
        self.stream_ref
    }
    /// The raw bytes. Always available.
    pub fn raw_bytes(&self) -> &[u8] {
        self.payload.bytes()
    }
    /// A typed slice view. `Err(FormatMismatch)` if the byte length is not a
    /// whole number of `size_of::<T>()`, or on misalignment: the payload's
    /// `data_offset` is wire-provided and only as aligned as it happens to
    /// be, not guaranteed a multiple of `align_of::<T>()` (only the
    /// underlying buffer's own start is guaranteed 16-byte aligned; see
    /// `bytes.rs`) - `try_cast_slice` turns that into this `Err`, never a
    /// panic.
    pub fn as_slice<T: Pod>(&self) -> Result<&[T], Error> {
        bytemuck::try_cast_slice(self.raw_bytes()).map_err(|_| Error::FormatMismatch {
            requested: std::mem::size_of::<T>(),
            actual: self.raw_bytes().len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytes::AlignedBuf;
    use std::sync::Arc;

    fn buf(bytes: &[u8]) -> Buffer {
        let b = Arc::new(AlignedBuf::from_bytes(bytes));
        Buffer::from_parts(7, Payload::new(b, 0, bytes.len()))
    }

    fn heap(bytes: &[u8]) -> Heap {
        let b = Arc::new(AlignedBuf::from_bytes(bytes));
        Heap::from_parts(3, Payload::new(b, 0, bytes.len()))
    }

    #[test]
    fn as_slice_reads_a_u32_ramp() {
        let ramp: Vec<u32> = (0..8).collect();
        let bytes: Vec<u8> = bytemuck::cast_slice(&ramp).to_vec();
        let b = buf(&bytes);
        assert_eq!(b.as_slice::<u32>().unwrap(), ramp.as_slice());
        assert_eq!(b.stream_ref(), 7);
        assert_eq!(b.raw_bytes(), bytes.as_slice());
    }

    #[test]
    fn as_slice_errors_on_length_not_a_multiple() {
        // 6 bytes is not a whole number of u32 (4-byte) elements.
        let bytes: Vec<u8> = vec![0; 6];
        let b = buf(&bytes);
        assert!(matches!(
            b.as_slice::<u32>(),
            Err(Error::FormatMismatch {
                requested: 4,
                actual: 6
            })
        ));
    }

    #[test]
    fn heap_as_slice_reads_a_u32_ramp() {
        let ramp: Vec<u32> = (0x2000..0x2004).collect();
        let bytes: Vec<u8> = bytemuck::cast_slice(&ramp).to_vec();
        let h = heap(&bytes);
        assert_eq!(h.as_slice::<u32>().unwrap(), ramp.as_slice());
        assert_eq!(h.stream_ref(), 3);
        assert_eq!(h.raw_bytes(), bytes.as_slice());
    }

    #[test]
    fn heap_as_slice_errors_on_length_not_a_multiple() {
        let bytes: Vec<u8> = vec![0; 6];
        let h = heap(&bytes);
        assert!(matches!(
            h.as_slice::<u32>(),
            Err(Error::FormatMismatch {
                requested: 4,
                actual: 6
            })
        ));
    }
}
