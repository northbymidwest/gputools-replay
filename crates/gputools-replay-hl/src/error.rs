//! The crate's error type: substrate failures re-exported alongside the
//! high-level layer's own decode/format errors.

use gputools_replay::{FetchError, SessionError};

/// Errors from the high-level layer.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A session/open failure from the substrate.
    #[error(transparent)]
    Session(#[from] SessionError),
    /// A fetch failure from the substrate.
    #[error(transparent)]
    Fetch(#[from] FetchError),
    /// Requested element size does not match the format's stride.
    #[error("format mismatch: requested {requested} B, format is {actual} B")]
    FormatMismatch {
        /// The element size the caller asked for, in bytes.
        requested: usize,
        /// The format's actual stride, in bytes.
        actual: usize,
    },
    /// Typed pixel access on a non-`Color`/single-aspect format.
    #[error("typed access unavailable: {0}")]
    WrongCategory(&'static str),
    /// Rows are padded; use `rows()` not `pixels()`.
    #[error("rows are padded (bytes_per_row {bytes_per_row} > row {row_len}); use rows()")]
    Padded {
        /// The stride between rows, in bytes, as stored.
        bytes_per_row: u32,
        /// The unpadded row length, in bytes.
        row_len: u32,
    },
    /// A format the table does not describe.
    #[error("unknown pixel format {0:#x}")]
    UnknownFormat(u32),
    /// The payload sub-slice a zero-copy cast would read from is not a
    /// multiple of the requested element type's alignment. `AlignedBuf`
    /// guarantees the *underlying buffer*'s start is 16-byte aligned, but a
    /// sub-slice at an arbitrary `data_offset` (or per-row stride) is only
    /// as aligned as that offset happens to be - both come off the wire, so
    /// neither is trustworthy. This is a decode error, not a panic: never a
    /// `bytemuck` alignment panic.
    #[error("payload offset not {align}-aligned for the requested element type")]
    Misaligned {
        /// The requested element type's required alignment, in bytes.
        align: usize,
    },
    /// A payload too short for the geometry/field being decoded.
    #[error("payload truncated: shorter than the declared geometry requires")]
    Truncated,
    /// The pipeline nested bplist did not parse to the expected shape.
    #[error("bad pipeline payload: {0}")]
    BadPipeline(String),
}
