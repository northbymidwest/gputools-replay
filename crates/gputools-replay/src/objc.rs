//! Typed ObjC interfaces for the RE'd `GTReplay*` request/batch classes
//! `fetch.rs` constructs. `extern_class!` + `extern_methods!` give the
//! setters a single typed source of truth instead of ad hoc `msg_send!`
//! calls sprinkled through the fetch builders; the classdump encodings that
//! justify each declaration (read off the live classes - probes,
//! session.rs, and dossier 00 "Texture format & shape coverage") live here
//! now instead of at each call site.
//!
//! The six fetch-request classes share a real, RE'd common superclass:
//! MEASURED via `class_getSuperclass`, every one is
//! `GTReplayFetch<Kind> -> GTReplayRequest -> NSObject`. They are declared
//! `super(GTReplayRequest)` here so the type hierarchy mirrors the live one
//! and a batch's element type can be the precise `GTReplayRequest` rather
//! than a bare `AnyObject`. `GTReplayRequestBatch` and the service/response
//! classes are direct `NSObject` subclasses (also measured).
//!
//! Also here: `GTReplayRequestBatch`'s `-setCompletionHandler:` and
//! `GTMTLReplayService`'s `-fetch:`, the two dispatch sends `fetch.rs` uses
//! to kick off an async fetch, plus `GTReplayResponse`'s two response
//! getters `fetch::read_response` reads (`-data`, `-error`), both plain +0
//! autoreleased getters objc2 hands back as `Option<Retained<...>>`. The
//! rest of what `read_response` sends stays out of scope: the
//! `-respondsToSelector:` guard is NSObject protocol and stays raw.

use crate::FetchError;
use crate::request::{DispatchUid, GTRegion, GTSize};
use block2::Block;
use gputools_replay_sys::client::GTMTLReplayClient;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, NSObject, NSObjectProtocol};
use objc2::{AnyThread, ClassType, extern_class, extern_methods, msg_send};
use objc2_foundation::{NSArray, NSData, NSError, NSURL};
use std::ffi::CStr;

extern_class!(
    /// The common superclass of every fetch-request class (MEASURED: each
    /// `GTReplayFetch*` derives from it, `-> NSObject`). This crate never
    /// constructs or messages it directly; it exists so a batch's requests
    /// have a precise shared element type (`NSArray<GTReplayRequest>`).
    #[unsafe(super(NSObject))]
    pub(crate) struct GTReplayRequest;
);

extern_class!(
    /// A texture fetch request.
    #[unsafe(super(GTReplayRequest))]
    pub(crate) struct GTReplayFetchTexture;
);

extern_class!(
    /// A buffer fetch request (streamRef-keyed).
    #[unsafe(super(GTReplayRequest))]
    pub(crate) struct GTReplayFetchBuffer;
);

extern_class!(
    /// A heap fetch request (streamRef-keyed).
    #[unsafe(super(GTReplayRequest))]
    pub(crate) struct GTReplayFetchHeap;
);

extern_class!(
    /// An acceleration-structure fetch request (streamRef-keyed).
    #[unsafe(super(GTReplayRequest))]
    pub(crate) struct GTReplayFetchAccelerationStructure;
);

extern_class!(
    /// A pipeline-binaries fetch request (streamRef-keyed).
    #[unsafe(super(GTReplayRequest))]
    pub(crate) struct GTReplayFetchPipelineBinaries;
);

extern_class!(
    /// A wireframe render request (dispatch-keyed).
    #[unsafe(super(GTReplayRequest))]
    pub(crate) struct GTReplayFetchWireframe;
);

extern_class!(
    /// The batch a fetch is dispatched through: `-setRequests:` and
    /// `-setCompletionHandler:`.
    #[unsafe(super(NSObject))]
    pub(crate) struct GTReplayRequestBatch;
);

extern_class!(
    /// The replayer service `fetch.rs` sends `-fetch:` to. Constructed once,
    /// in `session.rs`, via `+alloc`/`-initWithContext:` on exactly this
    /// class (see `Session::open`), then leaked into a raw pointer for the
    /// life of the session - `fetch.rs` casts that pointer back to this type
    /// at each call site.
    #[unsafe(super(NSObject))]
    pub(crate) struct GTMTLReplayService;
);

extern_class!(
    /// The response object a fetch's completion handler is handed (the block
    /// argument in `fetch::fetch_with_handler`). `fetch::read_response` reads
    /// two getters off it - `-error` then `-data` - after a
    /// `-respondsToSelector:` guard confirms it answers both.
    #[unsafe(super(NSObject))]
    pub(crate) struct GTReplayResponse;
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
    pub(crate) struct GTReplayRequestToken;
);

impl GTReplayFetchTexture {
    extern_methods!(
        // SAFETY: read off the live class (probes, session.rs ~835-849):
        // `-setStreamRef:` is `v24@0:8Q16`.
        #[unsafe(method(setStreamRef:))]
        pub(crate) fn set_stream_ref(&self, stream_ref: u64);

        // SAFETY: `-setSize:` is `v40@0:8{GTSize=QQQ}16`.
        #[unsafe(method(setSize:))]
        pub(crate) fn set_size(&self, size: GTSize);

        // SAFETY: `-setRegion:` is
        // `v64@0:8{GTRegion={GTPoint3D=QQQ}{GTSize=QQQ}}16`.
        #[unsafe(method(setRegion:))]
        pub(crate) fn set_region(&self, region: GTRegion);

        // SAFETY: `-setPlane:` is `v20@0:8I16` (`u32`).
        #[unsafe(method(setPlane:))]
        pub(crate) fn set_plane(&self, plane: u32);

        // SAFETY: `-setDepth:` is `v20@0:8I16` (`u32`). Always called with
        // `1`, never a caller-controlled value - see
        // `fetch::build_texture_batch`.
        #[unsafe(method(setDepth:))]
        pub(crate) fn set_depth(&self, depth: u32);

        // SAFETY: `-setSlice:` is `v20@0:8I16` (`u32`), read off the live
        // class (dossier 00 "Texture format & shape coverage").
        #[unsafe(method(setSlice:))]
        pub(crate) fn set_slice(&self, slice: u32);

        // SAFETY: `-setLevel:` is `v20@0:8I16` (`u32`), same source as
        // `-setSlice:`.
        #[unsafe(method(setLevel:))]
        pub(crate) fn set_level(&self, level: u32);
    );
}

impl GTReplayFetchBuffer {
    extern_methods!(
        // SAFETY: `-setStreamRef:` is `v24@0:8Q16` (u64), verified live for
        // every graduated fetch/decode class (probes, `fetch_raw`).
        #[unsafe(method(setStreamRef:))]
        pub(crate) fn set_stream_ref(&self, stream_ref: u64);
    );
}

impl GTReplayFetchHeap {
    extern_methods!(
        // SAFETY: as `GTReplayFetchBuffer::set_stream_ref`.
        #[unsafe(method(setStreamRef:))]
        pub(crate) fn set_stream_ref(&self, stream_ref: u64);
    );
}

impl GTReplayFetchAccelerationStructure {
    extern_methods!(
        // SAFETY: as `GTReplayFetchBuffer::set_stream_ref`.
        #[unsafe(method(setStreamRef:))]
        pub(crate) fn set_stream_ref(&self, stream_ref: u64);
    );
}

impl GTReplayFetchPipelineBinaries {
    extern_methods!(
        // SAFETY: as `GTReplayFetchBuffer::set_stream_ref`.
        #[unsafe(method(setStreamRef:))]
        pub(crate) fn set_stream_ref(&self, stream_ref: u64);
    );
}

/// Common interface for the four streamRef-keyed fetch classes
/// (buffer/heap/accel/pipeline), so `fetch::build_streamref_batch` can stay
/// one generic function instead of four near-identical copies. Each impl
/// forwards to the class's own inherent `set_stream_ref` (declared above via
/// `extern_methods!`) by explicit UFCS, not `self.set_stream_ref(..)`, so it
/// is unambiguous which method runs.
pub(crate) trait StreamRefFetch: ClassType + AnyThread {
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
        pub(crate) fn set_dispatch_uid(&self, dispatch_uid: DispatchUid);

        // SAFETY: `-setSolid:` is `v20@0:8B16` (BOOL).
        #[unsafe(method(setSolid:))]
        pub(crate) fn set_solid(&self, solid: bool);
    );
}

impl GTReplayRequestBatch {
    extern_methods!(
        // SAFETY: `-setRequests:` is `v24@0:8@16`, and `requests` is
        // declared `@"NSArray"` (element type erased in the ObjC encoding;
        // every element is in fact a `GTReplayRequest`, MEASURED).
        #[unsafe(method(setRequests:))]
        pub(crate) fn set_requests(&self, requests: &NSArray<GTReplayRequest>);

        // SAFETY: `-setCompletionHandler:` is `v24@0:8@?16`, a `copy`
        // property (probes, `fetch.rs`'s call-site disassembly): the batch
        // takes its own reference to the block, so a block reference that
        // only lives as long as the call is sound. The block's signature is
        // exactly one argument, `*mut GTReplayResponse` (the response) - see
        // `fetch::fetch_with_handler`'s doc for why that arity is
        // load-bearing (a block declared with more arguments would read
        // uninitialised registers as objects).
        #[unsafe(method(setCompletionHandler:))]
        pub(crate) fn set_completion_handler(&self, handler: &Block<dyn Fn(*mut GTReplayResponse)>);
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
        pub(crate) fn init_with_context(
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
        pub(crate) fn fetch(
            &self,
            batch: &GTReplayRequestBatch,
        ) -> Option<Retained<GTReplayRequestToken>>;

        // SAFETY: `-load:error:` is `B32@0:8@16^@24` - BOOL return, an
        // `NSURL` (NOT a `GTReplayLoadRequest`: passing one raises
        // `-[GTReplayLoadRequest scheme]: unrecognized selector`, MEASURED,
        // session.rs), and a standard `NSError**` out-pointer. objc2 maps
        // that out-pointer to `Option<&mut Option<Retained<NSError>>>`.
        #[unsafe(method(load:error:))]
        pub(crate) fn load(
            &self,
            url: &NSURL,
            error: Option<&mut Option<Retained<NSError>>>,
        ) -> bool;
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
        pub(crate) fn data(&self) -> Option<Retained<NSData>>;

        // SAFETY: `-error` is `@16@0:8` (no args, returns an object),
        // declared `@"NSError"` on the live class. Same +0 autoreleased
        // getter shape as `-data`; nil means no replayer error on this
        // response.
        #[unsafe(method(error))]
        pub(crate) fn error(&self) -> Option<Retained<NSError>>;
    );
}

/// `+alloc` then `-init` on `T`, guarding against `GPUToolsReplay` not
/// having loaded.
///
/// `T::alloc()` calls `T::class()`, which - under this crate's default,
/// non-`unstable-static-class` feature set - **panics** if the class isn't
/// registered (`ClassType::class`'s documented contract). Every class this
/// crate constructs is exported by the framework, which is linked, so it is
/// registered before `main`; a miss here means the framework itself did not
/// load, and that must surface as [`FetchError::Setup`], not a panic. So
/// `name` is checked with [`AnyClass::get`] first, unconditionally, before
/// `T::alloc()` is ever reached.
pub(crate) fn new_request<T: ClassType + AnyThread>(
    name: &CStr,
) -> Result<Retained<T>, FetchError> {
    if AnyClass::get(name).is_none() {
        return Err(FetchError::Setup(format!(
            "the {} class is not registered; GPUToolsReplay did not load",
            name.to_string_lossy()
        )));
    }
    // SAFETY: NSObject's designated initialiser, on a fresh allocation.
    let object: Option<Retained<T>> = unsafe { msg_send![T::alloc(), init] };
    object.ok_or_else(|| {
        FetchError::Setup(format!(
            "{} could not be constructed",
            name.to_string_lossy()
        ))
    })
}
