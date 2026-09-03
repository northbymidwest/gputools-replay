//! The fetch core: per-class batch builders, the non-blocking callback
//! primitive, and the `NSRunLoop` pump a blocking API composes on top of it.
//!
//! Ported from `probes/src/session.rs`'s `build_batch`, `RcBlock`
//! completion-handler wiring, `read_response` and `wait_for` - the proven,
//! validated reference (see that module's doc comment for the deeper
//! rationale any `// SAFETY:` comment here does not repeat).
//!
//! # The blocking/non-blocking seam
//!
//! probes fused "build a batch, dispatch it, and block until it answers"
//! into one function per fetch class. This module deliberately does not:
//! async must later be exposed as a visibility change on top of the same
//! primitive, not a parallel API, so `wait_for` is split into two
//! independent pieces:
//!
//! - [`fetch_with_handler`] is the non-blocking primitive. It wraps a
//!   handler in the completion-handler block, sends `-fetch:`, and returns
//!   the [`Token`] as soon as the fetch is dispatched. It never pumps the
//!   run loop and never blocks.
//! - [`pump_until`] is the run-loop pump alone, taken verbatim from
//!   `wait_for`'s loop body.
//!
//! A blocking fetch (a later task) composes them:
//!
//! ```text
//! let outcome = new_outcome();
//! let token = fetch_with_handler(session, &batch, store_into(&outcome))?;
//! pump_until(&outcome, timeout)
//! ```

use crate::FetchError;
use crate::objc::{
    GTMTLReplayService, GTReplayFetchAccelerationStructure, GTReplayFetchBuffer, GTReplayFetchHeap,
    GTReplayFetchPipelineBinaries, GTReplayFetchTexture, GTReplayFetchWireframe, GTReplayRequest,
    GTReplayRequestBatch, GTReplayRequestToken, GTReplayResponse, StreamRefFetch, new_request,
};
use crate::reply::RawReply;
use crate::request::{GTRegion, TextureRequest, WireframeRequest};
use crate::session::Session;
use crate::util::truncate;
use block2::RcBlock;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::NSObjectProtocol;
use objc2::{ClassType, sel};
use objc2_foundation::{NSArray, NSData, NSDate, NSDefaultRunLoopMode, NSError, NSRunLoop};
use std::ffi::CStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A built `GTReplayRequestBatch`, ready to `-fetch:`. Opaque and
/// deliberately leaked (see [`build_batch_from`]): nothing establishes that
/// the replayer has finished with a batch once its fetch's completion
/// handler has fired, and releasing it would be a use-after-free that no
/// test could reliably catch (probes: `fetch_textures` et al.
/// `std::mem::forget(batch)`).
pub(crate) struct RequestBatch {
    batch: *mut GTReplayRequestBatch,
}

/// An opaque handle to a dispatched fetch's `GTReplayRequestToken`. Leaked,
/// like the batch it answers, for the same reason: nothing establishes that
/// the replayer is done with it once the handler has fired, and on the
/// timeout path it demonstrably is not (probes, measured). Never freed, no
/// `Drop`.
pub(crate) struct Token(#[allow(dead_code)] *mut GTReplayRequestToken);

/// Builds the `GTReplayRequestBatch` around `objects`. The request objects
/// are held by the batch's `requests` array, which is a retaining property,
/// so only the batch itself needs an owner - and that owner is nobody: the
/// batch is leaked (see [`RequestBatch`]).
fn build_batch_from(objects: Vec<Retained<GTReplayRequest>>) -> Result<RequestBatch, FetchError> {
    let batch: Retained<GTReplayRequestBatch> = new_request(c"GTReplayRequestBatch")?;
    let array = NSArray::from_retained_slice(&objects);
    batch.set_requests(&array);
    Ok(RequestBatch {
        batch: Retained::into_raw(batch),
    })
}

/// Widens a typed, freshly built request object (`GTReplayFetchTexture` etc.)
/// to the `GTReplayRequest` common superclass `build_batch_from` collects
/// (MEASURED: every fetch class derives directly from `GTReplayRequest`).
/// Safe: a single `Retained::into_super`, exactly one step up that chain.
fn widen<T: ClassType<Super = GTReplayRequest> + 'static>(
    object: Retained<T>,
) -> Retained<GTReplayRequest> {
    Retained::into_super(object)
}

/// Builds a batch of `GTReplayFetchTexture` requests. `depth` is always 1,
/// never taken from `requests`: see [`TextureRequest`]'s doc.
pub(crate) fn build_texture_batch(requests: &[TextureRequest]) -> Result<RequestBatch, FetchError> {
    if requests.is_empty() {
        return Err(FetchError::EmptyBatch);
    }
    let mut objects = Vec::with_capacity(requests.len());
    for request in requests {
        let object: Retained<GTReplayFetchTexture> = new_request(c"GTReplayFetchTexture")?;
        let wire_region: GTRegion = request.region.into();
        object.set_stream_ref(request.stream_ref);
        object.set_size(wire_region.size);
        object.set_region(wire_region);
        object.set_plane(request.plane);
        // Always depth = 1, never the region's: `-depth` is the slice
        // count, and a 2D texture has one. depth = 0 returns nothing from
        // the replayer - the prior project's most expensive wrong
        // conclusion (probes, `build_batch`).
        object.set_depth(1);
        object.set_slice(request.slice);
        object.set_level(request.level);
        objects.push(widen(object));
    }
    build_batch_from(objects)
}

/// Builds a batch of `T` requests, setting only `-setStreamRef:` on each.
/// Shared by the four streamRef-keyed, non-texture fetch classes
/// (buffer/heap/accel/pipeline); ported from `probes::session::fetch_raw`.
fn build_streamref_batch<T: StreamRefFetch<Super = GTReplayRequest> + 'static>(
    class_name: &CStr,
    stream_refs: &[u64],
) -> Result<RequestBatch, FetchError> {
    if stream_refs.is_empty() {
        return Err(FetchError::EmptyBatch);
    }
    let mut objects = Vec::with_capacity(stream_refs.len());
    for &stream_ref in stream_refs {
        let object: Retained<T> = new_request(class_name)?;
        object.set_stream_ref(stream_ref);
        objects.push(widen(object));
    }
    build_batch_from(objects)
}

/// Builds a batch of `GTReplayFetchBuffer` requests (streamRef-keyed).
pub(crate) fn build_buffer_batch(stream_refs: &[u64]) -> Result<RequestBatch, FetchError> {
    build_streamref_batch::<GTReplayFetchBuffer>(c"GTReplayFetchBuffer", stream_refs)
}

/// Builds a batch of `GTReplayFetchHeap` requests (streamRef-keyed).
pub(crate) fn build_heap_batch(stream_refs: &[u64]) -> Result<RequestBatch, FetchError> {
    build_streamref_batch::<GTReplayFetchHeap>(c"GTReplayFetchHeap", stream_refs)
}

/// Builds a batch of `GTReplayFetchAccelerationStructure` requests
/// (streamRef-keyed).
pub(crate) fn build_accel_batch(stream_refs: &[u64]) -> Result<RequestBatch, FetchError> {
    build_streamref_batch::<GTReplayFetchAccelerationStructure>(
        c"GTReplayFetchAccelerationStructure",
        stream_refs,
    )
}

/// Builds a batch of `GTReplayFetchPipelineBinaries` requests
/// (streamRef-keyed).
pub(crate) fn build_pipeline_batch(stream_refs: &[u64]) -> Result<RequestBatch, FetchError> {
    build_streamref_batch::<GTReplayFetchPipelineBinaries>(
        c"GTReplayFetchPipelineBinaries",
        stream_refs,
    )
}

/// Builds a batch of `GTReplayFetchWireframe` requests (dispatch-keyed:
/// `-setDispatchUID:` + `-setSolid:`, no streamRef).
pub(crate) fn build_wireframe_batch(
    requests: &[WireframeRequest],
) -> Result<RequestBatch, FetchError> {
    if requests.is_empty() {
        return Err(FetchError::EmptyBatch);
    }
    let mut objects = Vec::with_capacity(requests.len());
    for request in requests {
        let object: Retained<GTReplayFetchWireframe> = new_request(c"GTReplayFetchWireframe")?;
        object.set_dispatch_uid(request.dispatch_uid);
        object.set_solid(request.solid);
        objects.push(widen(object));
    }
    build_batch_from(objects)
}

/// Reads the one argument a fetch's completion block is handed, turning it
/// into a parsed [`RawReply`] or a [`FetchError`]. Ported from
/// `probes::session::read_response`, extended to parse the extracted bytes
/// (`RawReply::parse`) rather than stop at raw `Vec<u8>`, since this crate
/// always wants the parsed envelope.
///
/// # Safety
///
/// `response` must be null or a live object.
unsafe fn read_response(response: *mut GTReplayResponse) -> Result<RawReply, FetchError> {
    if response.is_null() {
        return Err(FetchError::NoResponse);
    }
    // SAFETY: `response` is null-checked above and otherwise a live object
    // (this fn's contract), so reborrowing it as its class is sound. The
    // reborrow deliberately precedes the guard: `-respondsToSelector:` is an
    // NSObject-protocol message every objc object answers by dynamic
    // dispatch, independent of the object's real class, so asking it through
    // a typed `&GTReplayResponse` is sound even before that class is trusted.
    let resp: &GTReplayResponse = unsafe { &*response };
    // Every selector this function goes on to send (-data, -error) must be
    // guarded here: an ObjC `doesNotRecognizeSelector:` raise unwinding
    // through these frames is undefined behaviour, and a guard that covers
    // only some of the selectors sent is not a guard. `NSObjectProtocol`
    // gives the check as a safe, typed call (conformance in `objc.rs`).
    if !resp.respondsToSelector(sel!(data)) || !resp.respondsToSelector(sel!(error)) {
        return Err(FetchError::NoResponse);
    }
    // The guard confirmed both getters; reading them (declared in objc.rs)
    // is sound.
    let error: Option<Retained<NSError>> = resp.error();
    if let Some(error) = error {
        return Err(FetchError::Replayer {
            message: truncate(&error.localizedDescription().to_string()),
        });
    }
    let data: Option<Retained<NSData>> = resp.data();
    let bytes = match data {
        Some(data) => data.to_vec(),
        None => return Err(FetchError::NoData),
    };
    RawReply::parse(&bytes)
}

/// What a fetch's completion handler leaves for a waiter (via
/// [`store_into`]). `Result<RawReply, FetchError>` and nothing else: the
/// handler runs on a thread of the framework's own (probes, measured -
/// `NSThread` number 2, never the caller's), so no `Retained` may cross this
/// boundary.
pub(crate) type Outcome = Arc<Mutex<Option<Result<RawReply, FetchError>>>>;

/// A fresh, empty [`Outcome`] for one fetch.
pub(crate) fn new_outcome() -> Outcome {
    Arc::new(Mutex::new(None))
}

/// Builds the completion-handler closure a blocking fetch passes to
/// [`fetch_with_handler`]: stores the result into `outcome`. First fire
/// wins - the handler is documented nowhere, so nothing rules out a second
/// call, and the first result is the one a waiter may already have returned
/// (probes, `fetch_textures`).
pub(crate) fn store_into(outcome: &Outcome) -> impl FnOnce(Result<RawReply, FetchError>) + 'static {
    let sink = Arc::clone(outcome);
    move |result| {
        let mut slot = match sink.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if slot.is_none() {
            *slot = Some(result);
        }
    }
}

/// Sets `handler` as `batch`'s completion handler and sends `-fetch:`.
///
/// The non-blocking primitive: it never pumps the run loop and never
/// blocks, returning [`Token`] as soon as `-fetch:` has been dispatched.
/// `handler` fires later, on a thread of the framework's own, once the fetch
/// completes.
///
/// A replay error is reported out of band, through the session's error
/// observer, not through the fetch response itself - "a reply that decodes
/// cleanly while the replayer was complaining is not a success" (probes,
/// `fetch_textures`). So the observer is marked *before* `-fetch:` is sent
/// and checked *at completion time*, inside the block, after `read_response`
/// has run: an observer error reported for this fetch overrides an
/// otherwise-clean [`RawReply`], and a `read_response` failure is always
/// kept (a hard failure is a hard failure regardless of what the observer
/// says). This must happen at completion time, not synchronously after
/// `-fetch:` returns here: the replayer error this exists to catch is
/// reported *during* the async fetch, which has not run yet at the point
/// `-fetch:` returns.
pub(crate) fn fetch_with_handler(
    session: &Session,
    batch: &RequestBatch,
    handler: impl FnOnce(Result<RawReply, FetchError>) + 'static,
) -> Result<Token, FetchError> {
    // A libtest-spawned thread has no autorelease pool, and neither does a
    // Rust `main`. Without one, everything the framework autoreleases while
    // the batch's completion handler is wired up and `-fetch:` runs leaks,
    // one runtime warning per object (probes, `fetch_textures`).
    autoreleasepool(|_| {
        // Taken before the fetch is dispatched: the observer accumulates for
        // the life of the session, so only what it reports from here on is
        // this fetch's (probes: `fetch_textures`/`fetch_raw`/
        // `fetch_wireframe`). Checked inside the block below, at completion
        // time - see this function's doc.
        let mark = session.observer_error_count();
        // A cloned, `'static` handle to the observer's errors: the
        // completion block runs on a framework thread, well after this
        // function (and its `&Session` borrow) has returned, so it cannot
        // capture `session` itself. See `Session::observer_checker`'s doc
        // for why capturing this handle across that boundary is sound.
        let checker = session.observer_checker();

        // The block must be callable more than once (`RcBlock::new` requires
        // `Fn`), but `handler` is `FnOnce`: held in a `Mutex<Option<_>>` so
        // the first invocation can take and call it. First fire wins, as
        // above.
        let handler = Mutex::new(Some(handler));
        // The handler takes EXACTLY ONE argument. Established from probes'
        // call-site disassembly, not from a header:
        //   ldr x9, [x8, #0x10]!   ; block->invoke
        //   mov x1, x25            ; one argument
        //   blraa x9, x8
        // A block declared with more arguments would read uninitialised
        // registers as objects.
        let block = RcBlock::new(move |response: *mut GTReplayResponse| {
            // The framework's thread need not have a pool of its own, and
            // `-data` and `-error` both return autoreleased objects.
            let result = autoreleasepool(|_| {
                // SAFETY: the argument is the batch's `GTReplayResponse`,
                // per the call site below, and `read_response` checks it
                // answers every selector it goes on to send.
                unsafe { read_response(response) }
            });
            // The observer is authoritative over a clean-looking reply, per
            // this function's doc; a `read_response` failure is a hard
            // failure regardless and is never overridden by the observer's
            // silence.
            let result = match result {
                Ok(reply) => match checker(mark) {
                    Some(message) => Err(FetchError::Replayer { message }),
                    None => Ok(reply),
                },
                Err(e) => Err(e),
            };
            let taken = match handler.lock() {
                Ok(mut guard) => guard.take(),
                Err(poisoned) => poisoned.into_inner().take(),
            };
            if let Some(f) = taken {
                f(result);
            }
        });
        // SAFETY: `batch.batch` is a `*mut GTReplayRequestBatch` produced by
        // `build_batch_from` (`Retained::into_raw` over a value
        // `new_request::<GTReplayRequestBatch>` constructed), and outlives
        // this call (see `RequestBatch`'s doc: it is leaked, never freed).
        // Reborrowing the raw pointer as `&GTReplayRequestBatch` is sound;
        // see `objc.rs` for `-setCompletionHandler:`'s SAFETY (arity, copy
        // semantics).
        let typed_batch: &GTReplayRequestBatch = unsafe { &*batch.batch };
        typed_batch.set_completion_handler(&block);

        // SAFETY: `session.service()` is a `*mut GTMTLReplayService` produced
        // by `Session::open` (`+alloc`/`-initWithContext:` on that class,
        // then leaked - see `session.rs`), and outlives this call for the
        // same reason. Reborrowing the raw pointer as `&GTMTLReplayService`
        // is sound; see `objc.rs` for `-fetch:`'s SAFETY.
        let typed_service: &GTMTLReplayService = unsafe { &*session.service() };
        let token = typed_service.fetch(typed_batch);
        let token = token.ok_or(FetchError::NoToken)?;

        // Deliberately leaked, for the same reason `batch` is (see
        // `build_batch_from`): nothing establishes that the replayer has
        // finished with the token once the handler has fired, and on the
        // timeout path it demonstrably has not (probes, measured).
        // Releasing it would be a use-after-free that no test could
        // reliably catch.
        Ok(Token(Retained::into_raw(token)))
    })
}

/// How long one turn of the run loop may block. Short enough that a caller's
/// deadline is honoured to within a pump, long enough not to spin (probes,
/// `PUMP_SECONDS`).
const PUMP_SECONDS: f64 = 0.02;

/// One turn of this thread's run loop in the default mode. Ported from
/// `probes::session::pump_run_loop`.
///
/// **The mechanism is NOT established.** The completion fires on a framework
/// thread, and a run loop pumped on a non-main thread does not drain the
/// main dispatch queue. What it reliably provides is a bounded wait that
/// yields the CPU. Do not remove it on the theory that it does nothing until
/// what does deliver the completion is established.
fn pump_run_loop() {
    let run_loop = NSRunLoop::currentRunLoop();
    let until = NSDate::dateWithTimeIntervalSinceNow(PUMP_SECONDS);
    // SAFETY: reading a constant the framework exports.
    let mode = unsafe { NSDefaultRunLoopMode };
    if !run_loop.runMode_beforeDate(mode, &until) {
        // The loop had no input source to run, so it returned at once rather
        // than blocking until `until`. Without this the wait would spin a
        // core flat while the completion runs on another thread.
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Waits for `outcome` to be filled, pumping the run loop
/// ([`pump_run_loop`]) between checks, and returns the stored result.
///
/// The run-loop pump half of probes' `wait_for`, split out so a blocking API
/// composes it with [`fetch_with_handler`] rather than the two being fused
/// - see the module doc.
pub(crate) fn pump_until(outcome: &Outcome, timeout: Duration) -> Result<RawReply, FetchError> {
    let deadline = Instant::now() + timeout;
    loop {
        {
            let mut slot = match outcome.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Some(result) = slot.take() {
                return result;
            }
        }
        if Instant::now() >= deadline {
            return Err(FetchError::Timeout);
        }
        // One pool per turn: unlike probes' `wait_for` (one pool around the
        // whole fused fetch-and-wait), this primitive can now be pumped
        // across many separate calls, so autoreleases are drained each turn
        // rather than accumulating for the entire wait.
        autoreleasepool(|_| pump_run_loop());
    }
}

/// The typed, blocking fetch methods. Each is a thin composition of this
/// module's non-blocking primitives - build the typed batch, dispatch it
/// with a handler that stores into a fresh `Outcome`, pump the run loop
/// until it is filled or the timeout elapses, then check the observer
/// before decoding.
///
/// The observer check is unconditional, run against `result` rather than
/// short-circuited by it (probes: `fetch_textures`): an observer error
/// reported for this fetch overrides *both* a clean reply *and* a timeout,
/// because a completion that never fires is exactly what a replayer error
/// mid-fetch looks like from here. Only once the observer is confirmed
/// silent does `result?` get to propagate a bare [`FetchError::Timeout`].
impl Session {
    /// Fetches textures at natural or resampled size. Blocks up to
    /// `timeout`.
    ///
    /// # Errors
    ///
    /// Returns [`FetchError::EmptyBatch`] if `requests` is empty,
    /// [`FetchError::Timeout`] if no reply (and no observer error) arrives
    /// within `timeout`, [`FetchError::Replayer`] if the replayer reported
    /// an error attributed to this fetch, and the other [`FetchError`]
    /// variants for setup/parse failures.
    pub fn fetch_textures(
        &self,
        requests: &[crate::request::TextureRequest],
        timeout: Duration,
    ) -> Result<crate::reply::Reply<crate::reply::TextureRecord>, FetchError> {
        if requests.is_empty() {
            return Err(FetchError::EmptyBatch);
        }
        let batch = build_texture_batch(requests)?;
        let mark = self.observer_error_count();
        let outcome = new_outcome();
        fetch_with_handler(self, &batch, store_into(&outcome))?;
        let result = pump_until(&outcome, timeout);
        if let Some(message) = self.observer_first_error_since(mark) {
            return Err(FetchError::Replayer { message });
        }
        crate::reply::Reply::decode(result?)
    }

    /// Fetches raw buffer contents by streamRef. Blocks up to `timeout`.
    ///
    /// # Errors
    ///
    /// See [`Session::fetch_textures`].
    pub fn fetch_buffers(
        &self,
        stream_refs: &[u64],
        timeout: Duration,
    ) -> Result<crate::reply::Reply<crate::reply::BufferRecord>, FetchError> {
        if stream_refs.is_empty() {
            return Err(FetchError::EmptyBatch);
        }
        let batch = build_buffer_batch(stream_refs)?;
        let mark = self.observer_error_count();
        let outcome = new_outcome();
        fetch_with_handler(self, &batch, store_into(&outcome))?;
        let result = pump_until(&outcome, timeout);
        if let Some(message) = self.observer_first_error_since(mark) {
            return Err(FetchError::Replayer { message });
        }
        crate::reply::Reply::decode(result?)
    }

    /// Fetches raw heap contents by streamRef. Blocks up to `timeout`.
    ///
    /// # Errors
    ///
    /// See [`Session::fetch_textures`].
    pub fn fetch_heaps(
        &self,
        stream_refs: &[u64],
        timeout: Duration,
    ) -> Result<crate::reply::Reply<crate::reply::HeapRecord>, FetchError> {
        if stream_refs.is_empty() {
            return Err(FetchError::EmptyBatch);
        }
        let batch = build_heap_batch(stream_refs)?;
        let mark = self.observer_error_count();
        let outcome = new_outcome();
        fetch_with_handler(self, &batch, store_into(&outcome))?;
        let result = pump_until(&outcome, timeout);
        if let Some(message) = self.observer_first_error_since(mark) {
            return Err(FetchError::Replayer { message });
        }
        crate::reply::Reply::decode(result?)
    }

    /// Fetches raw acceleration-structure contents by streamRef. Blocks up
    /// to `timeout`.
    ///
    /// # Errors
    ///
    /// See [`Session::fetch_textures`].
    pub fn fetch_acceleration_structures(
        &self,
        stream_refs: &[u64],
        timeout: Duration,
    ) -> Result<crate::reply::Reply<crate::reply::AccelRecord>, FetchError> {
        if stream_refs.is_empty() {
            return Err(FetchError::EmptyBatch);
        }
        let batch = build_accel_batch(stream_refs)?;
        let mark = self.observer_error_count();
        let outcome = new_outcome();
        fetch_with_handler(self, &batch, store_into(&outcome))?;
        let result = pump_until(&outcome, timeout);
        if let Some(message) = self.observer_first_error_since(mark) {
            return Err(FetchError::Replayer { message });
        }
        crate::reply::Reply::decode(result?)
    }

    /// Fetches pipeline binaries by streamRef. Blocks up to `timeout`.
    ///
    /// # Errors
    ///
    /// See [`Session::fetch_textures`].
    pub fn fetch_pipeline_binaries(
        &self,
        stream_refs: &[u64],
        timeout: Duration,
    ) -> Result<crate::reply::Reply<crate::reply::PipelineRecord>, FetchError> {
        if stream_refs.is_empty() {
            return Err(FetchError::EmptyBatch);
        }
        let batch = build_pipeline_batch(stream_refs)?;
        let mark = self.observer_error_count();
        let outcome = new_outcome();
        fetch_with_handler(self, &batch, store_into(&outcome))?;
        let result = pump_until(&outcome, timeout);
        if let Some(message) = self.observer_first_error_since(mark) {
            return Err(FetchError::Replayer { message });
        }
        crate::reply::Reply::decode(result?)
    }

    /// Fetches rendered wireframe images by dispatch. Blocks up to
    /// `timeout`.
    ///
    /// # Errors
    ///
    /// See [`Session::fetch_textures`].
    pub fn fetch_wireframes(
        &self,
        requests: &[WireframeRequest],
        timeout: Duration,
    ) -> Result<crate::reply::Reply<crate::reply::WireframeRecord>, FetchError> {
        if requests.is_empty() {
            return Err(FetchError::EmptyBatch);
        }
        let batch = build_wireframe_batch(requests)?;
        let mark = self.observer_error_count();
        let outcome = new_outcome();
        fetch_with_handler(self, &batch, store_into(&outcome))?;
        let result = pump_until(&outcome, timeout);
        if let Some(message) = self.observer_first_error_since(mark) {
            return Err(FetchError::Replayer { message });
        }
        crate::reply::Reply::decode(result?)
    }
}
