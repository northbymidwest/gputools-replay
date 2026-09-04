//! Typed ObjC bindings for the RE'd `GTReplay*` request/batch/service/response
//! classes, plus the wire-format argument structs their setters take. Raw
//! bindings only: `extern_class!` + `extern_methods!` give each RE'd class and
//! method a single typed source of truth instead of ad hoc `msg_send!`, with
//! the classdump encodings that justify each declaration (read off the live
//! classes - probes, and dossier 00 "Texture format & shape coverage") on the
//! item. Policy (constructing objects, mapping nil to errors, session
//! lifecycle) lives in higher layers.
//!
//! The six fetch-request classes share a real, RE'd common superclass:
//! MEASURED via `class_getSuperclass`, every one is
//! `GTReplayFetch<Kind> -> GTReplayRequest -> NSObject`. They are declared
//! `super(GTReplayRequest)` here so the type hierarchy mirrors the live one
//! and a batch's element type can be the precise `GTReplayRequest` rather
//! than a bare `AnyObject`. `GTReplayRequestBatch` and the service/response
//! classes are direct `NSObject` subclasses (also measured).

use crate::client::GTMTLReplayClient;
use block2::Block;
use objc2::encode::{Encode, Encoding};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{AnyThread, ClassType, extern_class, extern_methods};
use objc2_foundation::{NSArray, NSData, NSError, NSURL};

/// The geometry a texture fetch request carries on the wire. Laid out to
/// match the type encodings the runtime reports for the setters, read off
/// the live class rather than guessed:
///
/// ```text
/// -setSize:    v40@0:8{GTSize=QQQ}16
/// -setRegion:  v64@0:8{GTRegion={GTPoint3D=QQQ}{GTSize=QQQ}}16
/// ```
///
/// A mismatch here is not a type error but a misaligned argument register, so
/// the `Encode` impls below spell the same encodings out exactly and objc2
/// checks them against the runtime in debug builds.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GTSize {
    pub width: u64,
    pub height: u64,
    pub depth: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GTPoint3D {
    pub x: u64,
    pub y: u64,
    pub z: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GTRegion {
    pub origin: GTPoint3D,
    pub size: GTSize,
}

// SAFETY: three `u64`s in declaration order, `#[repr(C)]`, no padding, which
// is exactly `{GTSize=QQQ}`.
unsafe impl Encode for GTSize {
    const ENCODING: Encoding =
        Encoding::Struct("GTSize", &[u64::ENCODING, u64::ENCODING, u64::ENCODING]);
}

// SAFETY: as above, `{GTPoint3D=QQQ}`.
unsafe impl Encode for GTPoint3D {
    const ENCODING: Encoding =
        Encoding::Struct("GTPoint3D", &[u64::ENCODING, u64::ENCODING, u64::ENCODING]);
}

// SAFETY: two `#[repr(C)]` structs of `u64`, so no padding is introduced
// between them either: `{GTRegion={GTPoint3D=QQQ}{GTSize=QQQ}}`.
unsafe impl Encode for GTRegion {
    const ENCODING: Encoding =
        Encoding::Struct("GTRegion", &[GTPoint3D::ENCODING, GTSize::ENCODING]);
}

/// The `dispatchUID` a dispatch-keyed fetch request carries: the ObjC
/// encoding `(?={?=ii}Q)` is an 8-byte UNION, read either as two `int32`s or
/// one `uint64`. It identifies the draw/dispatch whose debug data is being
/// fetched (dossier 00 "The fetch family", item 11).
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchUid(pub u64);

// SAFETY: `DispatchUid` is `#[repr(transparent)]` over a `u64` (8 bytes,
// 8-aligned), and this encoding is the exact `(?={?=ii}Q)` union the setter
// declares, so objc2's runtime encoding check accepts it.
unsafe impl Encode for DispatchUid {
    const ENCODING: Encoding = Encoding::Union(
        "?",
        &[
            Encoding::Struct("?", &[Encoding::Int, Encoding::Int]),
            Encoding::ULongLong,
        ],
    );
}

extern_class!(
    /// The common superclass of every fetch-request class (MEASURED: each
    /// `GTReplayFetch*` derives from it, `-> NSObject`). This crate never
    /// constructs or messages it directly; it exists so a batch's requests
    /// have a precise shared element type (`NSArray<GTReplayRequest>`).
    #[unsafe(super(NSObject))]
    pub struct GTReplayRequest;
);

extern_class!(
    /// A texture fetch request.
    #[unsafe(super(GTReplayRequest))]
    pub struct GTReplayFetchTexture;
);

extern_class!(
    /// A buffer fetch request (streamRef-keyed).
    #[unsafe(super(GTReplayRequest))]
    pub struct GTReplayFetchBuffer;
);

extern_class!(
    /// A heap fetch request (streamRef-keyed).
    #[unsafe(super(GTReplayRequest))]
    pub struct GTReplayFetchHeap;
);

extern_class!(
    /// An acceleration-structure fetch request (streamRef-keyed).
    #[unsafe(super(GTReplayRequest))]
    pub struct GTReplayFetchAccelerationStructure;
);

extern_class!(
    /// A pipeline-binaries fetch request (streamRef-keyed).
    #[unsafe(super(GTReplayRequest))]
    pub struct GTReplayFetchPipelineBinaries;
);

extern_class!(
    /// A wireframe render request (dispatch-keyed).
    #[unsafe(super(GTReplayRequest))]
    pub struct GTReplayFetchWireframe;
);

extern_class!(
    /// The batch a fetch is dispatched through: `-setRequests:` and
    /// `-setCompletionHandler:`.
    #[unsafe(super(NSObject))]
    pub struct GTReplayRequestBatch;
);

extern_class!(
    /// The replayer service `fetch.rs` sends `-fetch:` to. Constructed once,
    /// in `session.rs`, via `+alloc`/`-initWithContext:` on exactly this
    /// class (see `Session::open`), then leaked into a raw pointer for the
    /// life of the session - `fetch.rs` casts that pointer back to this type
    /// at each call site.
    #[unsafe(super(NSObject))]
    pub struct GTMTLReplayService;
);

extern_class!(
    /// The response object a fetch's completion handler is handed (the block
    /// argument in `fetch::fetch_with_handler`). `fetch::read_response` reads
    /// two getters off it - `-error` then `-data` - after a
    /// `-respondsToSelector:` guard confirms it answers both.
    #[unsafe(super(NSObject))]
    pub struct GTReplayResponse;
);

// SAFETY: `GTReplayResponse` is an NSObject subclass (super declared above,
// MEASURED), so it answers the NSObject protocol. This lets `read_response`
// call `-respondsToSelector:` as a safe, typed method instead of a raw send.
unsafe impl NSObjectProtocol for GTReplayResponse {}

extern_class!(
    /// The request token `-fetch:` returns: a live handle to the in-flight
    /// fetch (classdump-27.txt:3436, `instanceSize 32`, super `NSObject`). A
    /// hold-only marker here - the token owns cancel/wait/completion
    /// selectors on the live class, but this crate only keeps it alive for
    /// the life of the fetch and never messages it, so no methods are
    /// declared. See [`fetch::Token`] for why it is leaked rather than freed.
    #[unsafe(super(NSObject))]
    pub struct GTReplayRequestToken;
);

impl GTReplayFetchTexture {
    extern_methods!(
        // SAFETY: read off the live class (probes, session.rs ~835-849):
        // `-setStreamRef:` is `v24@0:8Q16`.
        #[unsafe(method(setStreamRef:))]
        pub fn set_stream_ref(&self, stream_ref: u64);

        // SAFETY: `-setSize:` is `v40@0:8{GTSize=QQQ}16`.
        #[unsafe(method(setSize:))]
        pub fn set_size(&self, size: GTSize);

        // SAFETY: `-setRegion:` is
        // `v64@0:8{GTRegion={GTPoint3D=QQQ}{GTSize=QQQ}}16`.
        #[unsafe(method(setRegion:))]
        pub fn set_region(&self, region: GTRegion);

        // SAFETY: `-setPlane:` is `v20@0:8I16` (`u32`).
        #[unsafe(method(setPlane:))]
        pub fn set_plane(&self, plane: u32);

        // SAFETY: `-setDepth:` is `v20@0:8I16` (`u32`). Always called with
        // `1`, never a caller-controlled value - see
        // `fetch::build_texture_batch`.
        #[unsafe(method(setDepth:))]
        pub fn set_depth(&self, depth: u32);

        // SAFETY: `-setSlice:` is `v20@0:8I16` (`u32`), read off the live
        // class (dossier 00 "Texture format & shape coverage").
        #[unsafe(method(setSlice:))]
        pub fn set_slice(&self, slice: u32);

        // SAFETY: `-setLevel:` is `v20@0:8I16` (`u32`), same source as
        // `-setSlice:`.
        #[unsafe(method(setLevel:))]
        pub fn set_level(&self, level: u32);
    );
}

impl GTReplayFetchBuffer {
    extern_methods!(
        // SAFETY: `-setStreamRef:` is `v24@0:8Q16` (u64), verified live for
        // every graduated fetch/decode class (probes, `fetch_raw`).
        #[unsafe(method(setStreamRef:))]
        pub fn set_stream_ref(&self, stream_ref: u64);
    );
}

impl GTReplayFetchHeap {
    extern_methods!(
        // SAFETY: as `GTReplayFetchBuffer::set_stream_ref`.
        #[unsafe(method(setStreamRef:))]
        pub fn set_stream_ref(&self, stream_ref: u64);
    );
}

impl GTReplayFetchAccelerationStructure {
    extern_methods!(
        // SAFETY: as `GTReplayFetchBuffer::set_stream_ref`.
        #[unsafe(method(setStreamRef:))]
        pub fn set_stream_ref(&self, stream_ref: u64);
    );
}

impl GTReplayFetchPipelineBinaries {
    extern_methods!(
        // SAFETY: as `GTReplayFetchBuffer::set_stream_ref`.
        #[unsafe(method(setStreamRef:))]
        pub fn set_stream_ref(&self, stream_ref: u64);
    );
}

/// Common interface for the four streamRef-keyed fetch classes
/// (buffer/heap/accel/pipeline), so `fetch::build_streamref_batch` can stay
/// one generic function instead of four near-identical copies. Each impl
/// forwards to the class's own inherent `set_stream_ref` (declared above via
/// `extern_methods!`) by explicit UFCS, not `self.set_stream_ref(..)`, so it
/// is unambiguous which method runs.
pub trait StreamRefFetch: ClassType + AnyThread {
    /// `-setStreamRef:`.
    fn set_stream_ref(&self, stream_ref: u64);
}

impl StreamRefFetch for GTReplayFetchBuffer {
    fn set_stream_ref(&self, stream_ref: u64) {
        GTReplayFetchBuffer::set_stream_ref(self, stream_ref)
    }
}

impl StreamRefFetch for GTReplayFetchHeap {
    fn set_stream_ref(&self, stream_ref: u64) {
        GTReplayFetchHeap::set_stream_ref(self, stream_ref)
    }
}

impl StreamRefFetch for GTReplayFetchAccelerationStructure {
    fn set_stream_ref(&self, stream_ref: u64) {
        GTReplayFetchAccelerationStructure::set_stream_ref(self, stream_ref)
    }
}

impl StreamRefFetch for GTReplayFetchPipelineBinaries {
    fn set_stream_ref(&self, stream_ref: u64) {
        GTReplayFetchPipelineBinaries::set_stream_ref(self, stream_ref)
    }
}

impl GTReplayFetchWireframe {
    extern_methods!(
        // SAFETY: `-setDispatchUID:` is `v24@0:8(?={?=ii}Q)16` (the
        // `DispatchUid` union).
        #[unsafe(method(setDispatchUID:))]
        pub fn set_dispatch_uid(&self, dispatch_uid: DispatchUid);

        // SAFETY: `-setSolid:` is `v20@0:8B16` (BOOL).
        #[unsafe(method(setSolid:))]
        pub fn set_solid(&self, solid: bool);
    );
}

impl GTReplayRequestBatch {
    extern_methods!(
        // SAFETY: `-setRequests:` is `v24@0:8@16`, and `requests` is
        // declared `@"NSArray"` (element type erased in the ObjC encoding;
        // every element is in fact a `GTReplayRequest`, MEASURED).
        #[unsafe(method(setRequests:))]
        pub fn set_requests(&self, requests: &NSArray<GTReplayRequest>);

        // SAFETY: `-setCompletionHandler:` is `v24@0:8@?16`, a `copy`
        // property (probes, `fetch.rs`'s call-site disassembly): the batch
        // takes its own reference to the block, so a block reference that
        // only lives as long as the call is sound. The block's signature is
        // exactly one argument, `*mut GTReplayResponse` (the response) - see
        // `fetch::fetch_with_handler`'s doc for why that arity is
        // load-bearing (a block declared with more arguments would read
        // uninitialised registers as objects).
        #[unsafe(method(setCompletionHandler:))]
        pub fn set_completion_handler(&self, handler: &Block<dyn Fn(*mut GTReplayResponse)>);
    );
}

impl GTMTLReplayService {
    extern_methods!(
        // SAFETY: `-initWithContext:` is the INIT-family constructor
        // `Session::open` sends to a fresh `+alloc` of this class. Its one
        // argument is the client pointer (`*mut GTMTLReplayClient`), the
        // struct `GTMTLReplayClient_init` fills in `open`; it is the receiver
        // `GTMTLReplayClient_init` established. `Allocated<Self>` receiver +
        // `Option<Retained<Self>>` return is objc2's init-method shape: the
        // returned object is +1 (init family consumes the allocation), and nil
        // is possible (`open` maps it to `SessionError::Replayer`).
        #[unsafe(method(initWithContext:))]
        pub fn init_with_context(
            this: Allocated<Self>,
            client: *mut GTMTLReplayClient,
        ) -> Option<Retained<Self>>;

        // SAFETY: `-fetch:` takes the batch - a bare request raises
        // `-[GTReplayFetchTexture requests]: unrecognized selector` (probes)
        // - and returns a `GTReplayRequestToken` (a plain getter-shaped
        // return: `Option<Retained<...>>`, nil handled as `FetchError::NoToken`
        // in `fetch.rs`). We only hold the token alive, never message it.
        // Asynchronous: the token is live until the completion handler fires.
        #[unsafe(method(fetch:))]
        pub fn fetch(&self, batch: &GTReplayRequestBatch)
        -> Option<Retained<GTReplayRequestToken>>;

        // SAFETY: `-load:error:` is `B32@0:8@16^@24` - BOOL return, an
        // `NSURL` (NOT a `GTReplayLoadRequest`: passing one raises
        // `-[GTReplayLoadRequest scheme]: unrecognized selector`, MEASURED,
        // session.rs), and a standard `NSError**` out-pointer. objc2 maps
        // that out-pointer to `Option<&mut Option<Retained<NSError>>>`.
        #[unsafe(method(load:error:))]
        pub fn load(&self, url: &NSURL, error: Option<&mut Option<Retained<NSError>>>) -> bool;
    );
}

impl GTReplayResponse {
    extern_methods!(
        // SAFETY: `-data` is `@16@0:8` (no args, returns an object), declared
        // `@"NSData"` on the live class. A plain getter, not the
        // new/copy/init/mutableCopy family, so it returns a +0 autoreleased
        // object - `Option<Retained<NSData>>` (nil is possible: see
        // `read_response`'s `FetchError::NoData`).
        #[unsafe(method(data))]
        pub fn data(&self) -> Option<Retained<NSData>>;

        // SAFETY: `-error` is `@16@0:8` (no args, returns an object),
        // declared `@"NSError"` on the live class. Same +0 autoreleased
        // getter shape as `-data`; nil means no replayer error on this
        // response.
        #[unsafe(method(error))]
        pub fn error(&self) -> Option<Retained<NSError>>;
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_uid_encoding_matches_the_setter() {
        assert_eq!(DispatchUid::ENCODING.to_string(), "(?={?=ii}Q)");
    }

    #[test]
    fn gtsize_encoding_matches_the_setter() {
        assert_eq!(GTSize::ENCODING.to_string(), "{GTSize=QQQ}");
    }

    #[test]
    fn gtregion_encoding_matches_the_setter() {
        assert_eq!(
            GTRegion::ENCODING.to_string(),
            "{GTRegion={GTPoint3D=QQQ}{GTSize=QQQ}}"
        );
    }
}
