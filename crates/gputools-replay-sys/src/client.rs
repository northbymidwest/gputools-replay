//! The opaque FFI types the bootstrap functions traffic in, and the client's
//! recorded type encoding. The Rust-side backing buffer and the measured byte
//! offsets that navigate these structs are non-FFI and live in
//! [`crate::layout`]. The derivation of each is recorded in `docs/HANDOFF.md`.

use objc2::encode::{Encoding, RefEncode};

/// An APR pool: field 0 of the client, typed `^{apr_pool_t}` by the
/// encoding of `-initWithContext:`. Opaque; only ever held by pointer.
#[repr(C)]
pub struct AprPool {
    _opaque: [u8; 0],
}

// SAFETY: only used behind a pointer, which is what ENCODING_REF describes.
// The struct name must be carried: objc2 verifies encodings against the
// runtime in debug builds, and `^v` does not match `^{apr_pool_t=...}`.
unsafe impl RefEncode for AprPool {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("apr_pool_t", &[]));
}

/// The replay controller, `^{GTMTLReplayController}` in the same encoding.
#[repr(C)]
pub struct GTMTLReplayController {
    _opaque: [u8; 0],
}

// SAFETY: as AprPool; a pointer to an opaque foreign struct carrying its name.
unsafe impl RefEncode for GTMTLReplayController {
    const ENCODING_REF: Encoding =
        Encoding::Pointer(&Encoding::Struct("GTMTLReplayController", &[]));
}

/// The replay context. Opaque here; its size is [`crate::layout::CLIENT_BUF_LEN`]
/// and the backing store is [`crate::layout::ClientBuffer`].
#[repr(C)]
pub struct GTMTLReplayClient {
    _opaque: [u8; 0],
}

// SAFETY: as AprPool; at this nesting level the runtime compares the name.
unsafe impl RefEncode for GTMTLReplayClient {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("GTMTLReplayClient", &[]));
}

/// The recorded type encoding of argument 2 of
/// `-[GTMTLReplayService initWithContext:]`. The runtime is the source of
/// truth; this is only the recorded copy, and the test compares the two on
/// every run. Copied from the prior project's runtime-verified constant, NOT
/// retyped from prose (an earlier transcription in a document was truncated by
/// 176 bytes; HANDOFF 2.3). [`crate::layout::CLIENT_BUF_LEN`] is derived from
/// this string.
pub const CLIENT_ENCODING: &str = "^{GTMTLReplayClient=^{apr_pool_t}\
^{GTMTLReplayController}Q{?=QQQdII}{?={?=II}IIfb1b1b1b1b28}@\
{GTMTLReplayWireframeRenderer=@{GTMTLReplayWireframeRenderPassDescriptor=@@SB[5C]}\
Q@@@@@@@@@@@@@@@@@}{GTMTLReplayOperationQueues=@@@}@@}";

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::runtime::AnyClass;
    use objc2::sel;

    /// CLIENT_BUF_LEN is sizeof of the struct THIS encoding describes, so
    /// the recorded encoding is the premise the whole derivation rests on.
    /// Reading it back from the live runtime turns that premise into a
    /// checked invariant: an OS update that changes GTMTLReplayClient fails
    /// here instead of silently corrupting a heap. This is the publication
    /// gate (HANDOFF section 7); it must never be feature-gated or skipped.
    #[test]
    fn the_encoding_the_client_size_was_derived_from_is_unchanged() {
        let class = AnyClass::get(c"GTMTLReplayService")
            .expect("GTMTLReplayService is not registered; is GPUToolsReplay linked?");
        let method = class
            .instance_method(sel!(initWithContext:))
            .expect("-[GTMTLReplayService initWithContext:] no longer exists");
        let arg = method
            .argument_type(2)
            .expect("-initWithContext: no longer takes an argument");
        assert_eq!(
            arg.to_str().expect("type encoding was not UTF-8"),
            CLIENT_ENCODING,
            "GTMTLReplayClient's layout has changed. CLIENT_BUF_LEN was \
             derived from the recorded encoding and must be re-derived \
             before this crate hands the framework a buffer again. See \
             `docs/HANDOFF.md` for the derivation procedure."
        );
    }
}
