//! The only unsafe module. Drives Apple's private GPUToolsReplay framework
//! in-process. No parsing happens here: this returns raw reply bytes and
//! nothing else, so that everything which can be silently wrong is checked
//! by safe, unit-tested code elsewhere.
//!
//! This module mutates no process state of its own. In particular it does not
//! set `MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX`: `std::env::set_var` is
//! sound only while no other thread may read the environment, and that is not
//! an obligation a safe `pub fn` can carry. A probe binary sets it as the
//! first statement of `main` (see `crate::guard::set_unlock_env`), where the
//! precondition is genuinely checkable; this module reads it back via
//! `gputools_replay_sys::env::unlock_env_ok` and refuses to open a session
//! without it.
#![allow(unsafe_code)]

use block2::RcBlock;
use gputools_replay_sys::client::{
    AprPool, ClientBuffer, GTMTLReplayClient, GTMTLReplayController,
};
use gputools_replay_sys::ffi::{
    GTMTLReplayClient_init, GTMTLReplayController_init, GTMTLReplayController_playAll,
    GTMTLReplayController_playTo, GTMTLReplayController_rewind,
    GTMTLReplayErrorHandling_initWithObserver, apr_initialize, apr_pool_create_ex,
};
use objc2::encode::{Encode, Encoding};
use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol};
use objc2::{AnyThread, DefinedClass, define_class, msg_send, sel};
use objc2_foundation::{
    NSArray, NSData, NSDate, NSDefaultRunLoopMode, NSError, NSRunLoop, NSString, NSURL,
};
use std::ffi::c_int;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("apr_initialize failed with {code}")]
    AprInit { code: c_int },
    #[error("apr_pool_create_ex failed with {code}")]
    AprPool { code: c_int },
    /// Distinct from `ServiceInit`: this means the framework itself is not
    /// there, not that constructing the service failed.
    #[error("the GTMTLReplayService class is not registered; GPUToolsReplay did not load")]
    ServiceClassMissing,
    #[error("GTMTLReplayService could not be constructed")]
    ServiceInit,
    #[error("load: returned NO for {bundle}")]
    LoadFailed { bundle: String },
    #[error("the replayer reported an error: {message}")]
    ReplayerError { message: String },
    #[error("bundle path {0} is not valid UTF-8")]
    BadPath(String),
    /// The replayer's state is process-global (one APR pool, one controller,
    /// one error observer), and a second bootstrap fails with a flood of
    /// "Buffer creation failed" before aborting the process. Measured.
    #[error("a replay session has already been opened in this process; only one is possible")]
    AlreadyOpen,
    /// The batch would be sent, and the replayer answers it with an archive
    /// carrying no records at all. Refused here so that the caller learns it
    /// asked for nothing, rather than reading an empty table as "no textures".
    #[error("a fetch was asked for with no requests")]
    EmptyBatch,
    /// Separate from `ServiceClassMissing` for the same reason it is separate
    /// from `ServiceInit`: this says the class is absent, not that it failed.
    #[error("the {name} class is not registered; GPUToolsReplay did not load")]
    FetchClassMissing { name: &'static str },
    #[error("{name} could not be constructed")]
    FetchObjectInit { name: &'static str },
    #[error("fetch: returned no request token, so no fetch was started")]
    NoToken,
    #[error(
        "the fetch did not complete within {seconds}s; the replayer's completion \
         handler never fired"
    )]
    FetchTimedOut { seconds: u64 },
    /// The completion block fired with nil, or with something that is not a
    /// `GTReplayResponse`. Named rather than sent `-data` regardless, because
    /// an ObjC raise unwinding through these frames is undefined behaviour.
    #[error("the fetch completion fired without a GTReplayResponse")]
    NoResponse,
    #[error("the replayer reported an error for the fetch: {message}")]
    FetchFailed { message: String },
    #[error("the fetch response carried no data")]
    NoData,
    /// See `gputools_replay_sys::env` for why this crate only ever verifies
    /// the unlock variable and never sets it.
    #[error(transparent)]
    UnlockEnv(#[from] gputools_replay_sys::env::UnlockEnvError),
    /// See `crate::guard` for why a bundle's shape is checked before `load:`
    /// ever sees it: a missing or malformed bundle SIGSEGVs the replayer
    /// rather than returning an error.
    #[error(transparent)]
    Guard(#[from] crate::guard::GuardError),
}

/// Set once the process-global bootstrap has been attempted. Never cleared:
/// a failed attempt has already touched the global state, so a retry is no
/// safer than a second success would be.
static BOOTSTRAPPED: AtomicBool = AtomicBool::new(false);

pub struct ObserverIvars {
    /// The replayer reports from queues of its own, so this is locked.
    errors: Mutex<Vec<String>>,
}

define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - ErrorObserver does not implement Drop.
    #[unsafe(super(NSObject))]
    #[ivars = ObserverIvars]
    pub struct ErrorObserver;

    unsafe impl NSObjectProtocol for ErrorObserver {}

    impl ErrorObserver {
        /// The one selector `GTMTLReplayErrorHandling` sends to its observer.
        /// The argument is an NSError; taken as `AnyObject` because nothing
        /// establishes that it is always one.
        #[unsafe(method(notifyError:))]
        fn notify_error(&self, error: Option<&AnyObject>) {
            let message = match error {
                Some(e) => {
                    // SAFETY: -description is NSObject protocol. Optional
                    // because an object is not obliged to answer with one.
                    let described: Option<Retained<NSString>> =
                        unsafe { msg_send![e, description] };
                    match described {
                        Some(d) => truncate(&d.to_string()),
                        None => "(no description)".to_owned(),
                    }
                }
                None => "(nil)".to_owned(),
            };
            self.record(message);
        }
    }
);

impl ErrorObserver {
    pub fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(ObserverIvars {
            errors: Mutex::new(Vec::new()),
        });
        // SAFETY: NSObject's designated initialiser, on a fresh allocation.
        unsafe { msg_send![super(this), init] }
    }

    pub fn record(&self, message: String) {
        // A poisoned lock still holds every error reported before the panic,
        // and dropping them here would restore exactly the silence the
        // observer exists to prevent.
        let mut errors = match self.ivars().errors.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        errors.push(message);
    }

    /// The first error reported, which is the proximate cause; later ones are
    /// usually its consequences.
    pub fn first_error(&self) -> Option<String> {
        self.first_error_since(0)
    }

    /// How many errors have been reported so far. Taken before an operation so
    /// that `first_error_since` can attribute an error to it.
    pub fn error_count(&self) -> usize {
        self.errors().len()
    }

    /// The first error reported after `mark`. A session outlives many fetches
    /// and this observer never forgets, so asking for the first error outright
    /// would blame every later fetch for the first one's failure.
    pub fn first_error_since(&self, mark: usize) -> Option<String> {
        self.errors().get(mark..)?.first().cloned()
    }

    fn errors(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        match self.ivars().errors.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// Replayer error descriptions embed a whole call stack. Keep enough to name
/// the failure without turning a CLI diagnostic into a page of frames.
fn truncate(s: &str) -> String {
    const LIMIT: usize = 512;
    if s.chars().nth(LIMIT).is_none() {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(LIMIT).collect();
    out.push_str(" ... (truncated)");
    out
}

/// The `dispatchUID` a dispatch-keyed fetch request carries: the ObjC encoding
/// `(?={?=ii}Q)` is an 8-byte UNION, read either as two `int32`s or one
/// `uint64`. It identifies the draw/dispatch whose debug data is being fetched
/// (dossier 00 "The fetch family", item 11).
#[repr(transparent)]
#[derive(Clone, Copy)]
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

pub struct Session {
    /// The replay context's backing store. Deliberately leaked rather than
    /// owned: the service is never released and the replayer's own queues keep
    /// this pointer, so freeing it when the Session drops would be a
    /// use-after-free. One session per process, so this is 312 bytes once.
    _client: *mut ClientBuffer,
    /// Retained so that errors reported after `load:` are still recorded;
    /// the framework holds its own strong reference as well.
    observer: Retained<ErrorObserver>,
    /// The receiver of `fetch:`.
    service: *mut AnyObject,
}

impl Session {
    pub fn open(bundle: &Path) -> Result<Self, SessionError> {
        // Checked before anything global is touched, so that these failures
        // leave the process able to open a session afterwards.
        crate::guard::check_bundle_shape(bundle)?;
        let path = bundle
            .to_str()
            .ok_or_else(|| SessionError::BadPath(bundle.display().to_string()))?;
        // Also checked before the one-shot guard below, so that a caller who
        // forgot the variable can set it and try again.
        gputools_replay_sys::env::unlock_env_ok()?;
        if BOOTSTRAPPED.swap(true, Ordering::SeqCst) {
            return Err(SessionError::AlreadyOpen);
        }

        // SAFETY, for the `GTMTLReplayClient_init` call below:
        // - Size: `ClientBuffer` is `sizeof(struct GTMTLReplayClient)` read
        //   off the type encoding the runtime reports for `-initWithContext:`,
        //   and that encoding is re-checked against the runtime by a test in
        //   `gputools_replay_sys::client`.
        // - Alignment: `ClientBuffer` is `align(16)`, which over-satisfies the
        //   type's own 8, so the pointers, `u64`s and atomic paths the callee
        //   takes over this memory are all correctly aligned.
        // - Lifetime: leaked, for the reasons given on the field, so it can
        //   neither move nor be freed while the framework holds the pointer.
        let backing: *mut ClientBuffer = Box::into_raw(Box::new(ClientBuffer::new_zeroed()));
        // SAFETY: `backing` was just allocated above via `Box::into_raw` and
        // nothing else has a reference to it yet, so a unique `&mut` through
        // the raw pointer is sound.
        let client: *mut GTMTLReplayClient = unsafe { (*backing).as_client_ptr() };

        // SAFETY: the sequence below is the one GPUToolsReplayService itself
        // performs; each call's arity is established, not assumed.
        unsafe {
            let rc = apr_initialize();
            if rc != 0 {
                return Err(SessionError::AprInit { code: rc });
            }
            let mut pool: *mut AprPool = std::ptr::null_mut();
            let rc = apr_pool_create_ex(
                &mut pool,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if rc != 0 || pool.is_null() {
                return Err(SessionError::AprPool { code: rc });
            }
            // Return value deliberately discarded: this initialises global
            // config from the environment and does NOT vend a controller.
            // Apple's own service discards it too. See the declaration's docs
            // and `docs/findings/01-playback.md`.
            let _ = GTMTLReplayController_init(pool);
            // This is what actually fills the client struct, controller
            // included (field 1).
            GTMTLReplayClient_init(client, pool);
        }

        // Register the observer BEFORE load:, or failures are silent.
        let observer = ErrorObserver::new();
        // SAFETY: one argument, an object with -notifyError:. The callee
        // objc_storeStrong's it, so it outlives this borrow of the pointer.
        unsafe {
            GTMTLReplayErrorHandling_initWithObserver(
                Retained::as_ptr(&observer).cast::<AnyObject>().cast_mut(),
            );
        }

        // SAFETY: the class is exported by the framework, which is linked, so
        // it is loaded before main. -initWithContext: takes the client pointer.
        let service: *mut AnyObject = unsafe {
            let cls =
                AnyClass::get(c"GTMTLReplayService").ok_or(SessionError::ServiceClassMissing)?;
            let alloc: Allocated<AnyObject> = msg_send![cls, alloc];
            let service: Option<Retained<AnyObject>> = msg_send![alloc, initWithContext: client];
            // Into a raw pointer, i.e. leaked: releasing a replay service is
            // not an operation this project has established as safe.
            service.map_or(std::ptr::null_mut(), Retained::into_raw)
        };
        if service.is_null() {
            return Err(SessionError::ServiceInit);
        }

        let loaded = load_bundle(service, path)?;
        // `load:` returning YES only reports that its dispatched block ran, so
        // the observer is the authority on whether the replay actually worked.
        if let Some(message) = observer.first_error() {
            // Probe-only escape hatch. gpudebug(1) reaches "Replayer ready"
            // on captures where the replayer reports a non-fatal load error
            // (seen: "Metal object creation failed" tagged
            // GTErrorKeyResourceUnused=true), so treating every observer
            // error as fatal is stricter than Apple's own tool. With
            // PROBE_TOLERATE_REPLAYER_ERRORS=1 the errors are printed and the
            // session continues; without it the existing fail-hard policy
            // stands. Not a library-level decision yet.
            if std::env::var_os("PROBE_TOLERATE_REPLAYER_ERRORS").is_some_and(|v| v == "1") {
                eprintln!(
                    "load reported {} error(s); continuing (PROBE_TOLERATE_REPLAYER_ERRORS=1). first: {message}",
                    observer.error_count()
                );
            } else {
                return Err(SessionError::ReplayerError { message });
            }
        }
        if !loaded {
            return Err(SessionError::LoadFailed {
                bundle: path.to_owned(),
            });
        }
        Ok(Session {
            _client: backing,
            observer,
            service,
        })
    }

    /// Drives playback to the end of the command stream.
    ///
    /// # Behavior
    ///
    /// **Unverified.** The signature (one argument, the controller) is
    /// established from disassembly (`docs/findings/01-playback.md`), so the
    /// call is safe with respect to arity and argument types, but this
    /// function has never been called against a live replayer. It exists to
    /// test the coverage-gap hypothesis in `docs/findings/00-texture-fetch.md`
    /// under a probe's own gate, not to be assumed correct here.
    pub fn play_all(&self) {
        // SAFETY: `self.controller` came from `GTMTLReplayController_init` on
        // this session's own pool in `open` and is valid for the session's
        // life (see the field doc comment). `GTMTLReplayController_playAll`
        // takes exactly one argument, the controller, per
        // `docs/findings/01-playback.md`; the BEHAVIOR of the call is not
        // established.
        unsafe { GTMTLReplayController_playAll(self.controller_in_client()) };
    }

    /// Drives playback forward from the controller's current command index up
    /// to (not including) `index`. Never rewinds: if the current index is
    /// already `>= index` this is a no-op (established control flow, see
    /// `docs/findings/01-playback.md`).
    ///
    /// # Behavior
    ///
    /// **Unverified**, for the same reason as [`Session::play_all`]: the
    /// signature is established, the effect on a live replayer is not.
    pub fn play_to(&self, index: u32) {
        // SAFETY: as `play_all`. `GTMTLReplayController_playTo` takes exactly
        // two arguments, the controller and a `u32` target command index, per
        // `docs/findings/01-playback.md`; the BEHAVIOR of the call is not
        // established.
        unsafe { GTMTLReplayController_playTo(self.controller_in_client(), index) };
    }

    /// Rewinds the controller: tears down, then restores initial state
    /// (inferred from control flow, see `docs/findings/01-playback.md`).
    ///
    /// # Behavior
    ///
    /// **Unverified**, for the same reason as [`Session::play_all`]: the
    /// signature is established, the effect on a live replayer is not.
    pub fn rewind(&self) {
        // SAFETY: as `play_all`. `GTMTLReplayController_rewind` takes exactly
        // one argument, the controller, per `docs/findings/01-playback.md`;
        // the BEHAVIOR of the call is not established.
        unsafe { GTMTLReplayController_rewind(self.controller_in_client()) };
    }
}

/// Sends `-load:error:`, which takes an **NSURL**: passing a
/// `GTReplayLoadRequest` raises `-[GTReplayLoadRequest scheme]: unrecognized
/// selector`. Measured, not inferred.
fn load_bundle(service: *mut AnyObject, path: &str) -> Result<bool, SessionError> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let mut error: Option<Retained<NSError>> = None;
    // SAFETY: `B32@0:8@16^@24` - BOOL return, an object and an out-pointer to
    // an object, read from the runtime's own method type encoding.
    let ok: bool = unsafe { msg_send![service, load: &*url, error: Some(&mut error)] };
    match error {
        // Probe-only escape hatch, see the observer check in `open`: gpudebug
        // reaches "Replayer ready" on captures where `-load:error:` hands back
        // a non-fatal error (seen: "Metal object creation failed", tagged
        // GTErrorKeyResourceUnused=true), so fail-hard here is stricter than
        // Apple's tool. With PROBE_TOLERATE_REPLAYER_ERRORS=1 the error is printed
        // and `ok` is returned as the load result.
        Some(e) if std::env::var_os("PROBE_TOLERATE_REPLAYER_ERRORS").is_some_and(|v| v == "1") => {
            eprintln!(
                "load:error: returned an error; continuing (PROBE_TOLERATE_REPLAYER_ERRORS=1): {}",
                truncate(&e.to_string())
            );
            Ok(ok)
        }
        Some(e) => Err(SessionError::ReplayerError {
            message: truncate(&e.localizedDescription().to_string()),
        }),
        None => Ok(ok),
    }
}

/// The geometry a fetch request carries. Both structs are laid out to match the
/// type encodings the runtime reports for the setters, read off the live class
/// rather than guessed:
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
struct GTSize {
    width: u64,
    height: u64,
    depth: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GTPoint3D {
    x: u64,
    y: u64,
    z: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GTRegion {
    origin: GTPoint3D,
    size: GTSize,
}

// SAFETY: three `u64`s in declaration order, `#[repr(C)]`, no padding, which is
// exactly `{GTSize=QQQ}`.
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

/// One texture to fetch.
#[derive(Debug, Clone, Copy)]
pub struct FetchRequest {
    /// The replayer's own key for the texture. Sparse: a capture's refs are
    /// nothing like `0..n`, so callers sweep a range and read back what
    /// answered.
    pub stream_ref: u64,
    /// The box to fit the texture into. **The fetch resamples**: it scales the
    /// texture to fit the requested region, preserving aspect ratio, so asking
    /// for a size that is not the texture's own returns resampled pixels, not
    /// a crop and not the natural image (measured, spike round 6).
    ///
    /// Zero is not empty. A zero `width` and `height` returns each texture at
    /// its **natural** size: measured on `small.gputrace`, a zero-region sweep
    /// returned 2880x2592 at 4 bytes per pixel for all four textures, matching
    /// the dimensions `gpudebug` reports for them. So zero is the way to ask
    /// for the real image, and it is deliberately not rejected here.
    pub width: u64,
    pub height: u64,
    /// The plane of a planar (e.g. YUV) texture. `0` for everything else.
    pub plane: u32,
}

/// How long one turn of the run loop may block. Short enough that a caller's
/// deadline is honoured to within a pump, long enough not to spin.
const PUMP_SECONDS: f64 = 0.02;

/// What the completion block leaves for the calling thread. `Vec<u8>` and
/// `String` and nothing else: the block runs on a thread of the framework's
/// own (measured - `NSThread` number 2, never the caller's), so no `Retained`
/// may cross this boundary.
type Outcome = Arc<Mutex<Option<Result<Vec<u8>, SessionError>>>>;

impl Session {
    /// Fetches every requested texture in one batch and returns the reply's
    /// raw bytes, which are an `NSKeyedArchiver` plist for
    /// [`crate::reply::parse_reply`]. Nothing is decoded here.
    ///
    /// One batch per sweep rather than one per texture: the replayer answers a
    /// batch with a single archive, so 2000 requests cost one round trip.
    ///
    /// `timeout` bounds the wait for the completion handler. Measured on
    /// `small.gputrace`: a 2000-request sweep at 64x64 completes in 44 ms, and
    /// a natural-size sweep moving 119 MB in 439 ms - three orders of magnitude
    /// of headroom, because the cost scales with the pixels a capture holds.
    /// Callers pass `Duration::from_secs(120)` for known captures; other
    /// campaign probes may need far longer. Past the deadline the handler is
    /// not late, it is not coming, and the wait is bounded so that a broken
    /// fetch is a named error rather than a process sitting at 0% CPU - which
    /// is exactly how `waitUntilCompleted` fails.
    pub fn fetch_textures(
        &self,
        requests: &[FetchRequest],
        timeout: Duration,
    ) -> Result<Vec<u8>, SessionError> {
        if requests.is_empty() {
            return Err(SessionError::EmptyBatch);
        }
        // A libtest-spawned thread has no autorelease pool, and neither does a
        // Rust `main`. Without one, everything the framework autoreleases while
        // the batch is built and while `fetch:` runs leaks, one runtime warning
        // per object. The block body has its own pool for the same reason: it
        // runs on a thread this one does not own.
        autoreleasepool(|_| {
            let batch = build_batch(requests)?;
            // Taken before the fetch starts: the observer accumulates for
            // the life of the session, so only what it reports from here on
            // is this fetch's.
            let mark = self.observer.error_count();

            let outcome: Outcome = Arc::new(Mutex::new(None));
            let sink = Arc::clone(&outcome);
            // The handler takes EXACTLY ONE argument. Established from the call
            // site's disassembly, not from a header:
            //   ldr x9, [x8, #0x10]!   ; block->invoke
            //   mov x1, x25            ; one argument
            //   blraa x9, x8
            // A block declared with more arguments would read uninitialised
            // registers as objects.
            let handler = RcBlock::new(move |response: *mut AnyObject| {
                // The framework's thread need not have a pool of its own, and
                // `-data` and `-error` both return autoreleased objects.
                let result = autoreleasepool(|_| {
                    // SAFETY: the argument is the batch's `GTReplayResponse`,
                    // per the call site above, and `read_response` checks it
                    // answers every selector it goes on to send.
                    unsafe { read_response(response) }
                });
                let mut slot = match sink.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                // First fire wins. The handler is documented nowhere, so
                // nothing rules out a second call, and the first result is
                // the one the waiter may already have returned.
                if slot.is_none() {
                    *slot = Some(result);
                }
            });
            // SAFETY: `-setCompletionHandler:` is `v24@0:8@?16`, a `copy`
            // property, so the batch takes its own reference to the block.
            let () = unsafe { msg_send![&*batch, setCompletionHandler: &*handler] };

            // SAFETY: `-fetch:` takes the batch - a bare request raises
            // `-[GTReplayFetchTexture requests]: unrecognized selector` - and
            // returns a `GTReplayRequestToken`. It is asynchronous: the token
            // is live until the handler fires.
            let token: Option<Retained<AnyObject>> =
                unsafe { msg_send![self.service, fetch: &*batch] };
            let token = token.ok_or(SessionError::NoToken)?;

            let result = wait_for(&outcome, timeout);

            // Deliberately leaked, for the same reason the service and the
            // client buffer are: nothing establishes that the replayer has
            // finished with the batch or its token once the handler has fired,
            // and on the timeout path it demonstrably has not. Releasing
            // either would be a use-after-free that no test could reliably
            // catch. The cost is one batch and its request objects per sweep,
            // in a process that opens one session and is not a long-running
            // server.
            std::mem::forget(batch);
            std::mem::forget(token);

            // Reported by the observer rather than by the fetch: a replay
            // error during a fetch arrives out of band, and a reply that
            // decodes cleanly while the replayer was complaining is not a
            // success.
            if let Some(message) = self.observer.first_error_since(mark) {
                // Probe-only, same escape hatch as in `open` and `load_bundle`:
                // an error attributed to this fetch (or to playback that ran
                // just before it) is printed and the reply is still returned,
                // so its bytes can be examined. Default remains fail-hard.
                if std::env::var_os("PROBE_TOLERATE_REPLAYER_ERRORS").is_some_and(|v| v == "1") {
                    eprintln!(
                        "replayer error attributed to this fetch; continuing (PROBE_TOLERATE_REPLAYER_ERRORS=1): {message}"
                    );
                } else {
                    return Err(SessionError::ReplayerError { message });
                }
            }
            result
        })
    }

    /// Fetch a batch of an arbitrary `GTReplayFetch*`/`GTReplayDecode*` request
    /// class, setting only `-setStreamRef:` on each, and return the raw reply
    /// bytes. Probe support for the non-texture fetch classes (dossiers 03/05),
    /// whose request shape is `streamRef + dispatchUID` with no texture-only
    /// setters. `dispatchUID` is left at its default. Returns the same
    /// `GTReplayResponse` `-data` blob `fetch_textures` does.
    pub fn fetch_raw(
        &self,
        class_name: &std::ffi::CStr,
        stream_refs: &[u64],
        timeout: Duration,
    ) -> Result<Vec<u8>, SessionError> {
        if stream_refs.is_empty() {
            return Err(SessionError::EmptyBatch);
        }
        autoreleasepool(|_| {
            let fetch_class = AnyClass::get(class_name).ok_or(SessionError::FetchClassMissing {
                name: "generic fetch class",
            })?;
            let batch_class =
                AnyClass::get(c"GTReplayRequestBatch").ok_or(SessionError::FetchClassMissing {
                    name: "GTReplayRequestBatch",
                })?;
            let mut objects: Vec<Retained<AnyObject>> = Vec::with_capacity(stream_refs.len());
            for &stream_ref in stream_refs {
                // SAFETY: NSObject alloc/init on a class that declares no other
                // designated initialiser; `-setStreamRef:` is `v24@0:8Q16`
                // (u64), verified live for every graduated fetch/decode class.
                let object: Option<Retained<AnyObject>> = unsafe {
                    let a: Allocated<AnyObject> = msg_send![fetch_class, alloc];
                    msg_send![a, init]
                };
                let object = object.ok_or(SessionError::FetchObjectInit {
                    name: "generic fetch object",
                })?;
                // Only resource-keyed fetch classes take -setStreamRef:. The
                // dispatch-keyed ones (threadgroup/imageblock/wireframe/
                // postvertex) do not, and sending it would be a hard error, so
                // fail cleanly and name the class instead.
                let responds: bool =
                    unsafe { msg_send![&*object, respondsToSelector: sel!(setStreamRef:)] };
                if !responds {
                    return Err(SessionError::FetchObjectInit {
                        name: "class is not streamRef-keyed (dispatch-keyed fetch)",
                    });
                }
                let () = unsafe { msg_send![&*object, setStreamRef: stream_ref] };
                objects.push(object);
            }
            let batch: Option<Retained<AnyObject>> = unsafe {
                let a: Allocated<AnyObject> = msg_send![batch_class, alloc];
                msg_send![a, init]
            };
            let batch = batch.ok_or(SessionError::FetchObjectInit {
                name: "GTReplayRequestBatch",
            })?;
            let array = NSArray::from_retained_slice(&objects);
            let () = unsafe { msg_send![&*batch, setRequests: &*array] };

            let mark = self.observer.error_count();
            let outcome: Outcome = Arc::new(Mutex::new(None));
            let sink = Arc::clone(&outcome);
            let handler = RcBlock::new(move |response: *mut AnyObject| {
                let result = autoreleasepool(|_| unsafe { read_response(response) });
                let mut slot = match sink.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                if slot.is_none() {
                    *slot = Some(result);
                }
            });
            let () = unsafe { msg_send![&*batch, setCompletionHandler: &*handler] };
            let token: Option<Retained<AnyObject>> =
                unsafe { msg_send![self.service, fetch: &*batch] };
            let token = token.ok_or(SessionError::NoToken)?;
            let result = wait_for(&outcome, timeout);
            std::mem::forget(batch);
            std::mem::forget(token);
            if let Some(message) = self.observer.first_error_since(mark) {
                if std::env::var_os("PROBE_TOLERATE_REPLAYER_ERRORS").is_some_and(|v| v == "1") {
                    eprintln!("fetch_raw: replayer error (tolerated): {message}");
                } else {
                    return Err(SessionError::ReplayerError { message });
                }
            }
            result
        })
    }

    /// Fetch `GTReplayFetchWireframe` for each dispatchUID (a dispatch-keyed
    /// fetch: `-setDispatchUID:` + `-setSolid:`, no streamRef). Probe support
    /// for item 11. Returns the raw reply bytes for the first dispatchUID that
    /// answers, or the last reply if none do.
    pub fn fetch_wireframe(
        &self,
        dispatch_uids: &[u64],
        timeout: Duration,
    ) -> Result<Vec<u8>, SessionError> {
        autoreleasepool(|_| {
            let cls = AnyClass::get(c"GTReplayFetchWireframe").ok_or(
                SessionError::FetchClassMissing {
                    name: "GTReplayFetchWireframe",
                },
            )?;
            let batch_class =
                AnyClass::get(c"GTReplayRequestBatch").ok_or(SessionError::FetchClassMissing {
                    name: "GTReplayRequestBatch",
                })?;
            let mut objects: Vec<Retained<AnyObject>> = Vec::with_capacity(dispatch_uids.len());
            for &uid in dispatch_uids {
                let object: Option<Retained<AnyObject>> = unsafe {
                    let a: Allocated<AnyObject> = msg_send![cls, alloc];
                    msg_send![a, init]
                };
                let object = object.ok_or(SessionError::FetchObjectInit {
                    name: "GTReplayFetchWireframe",
                })?;
                // SAFETY: `-setDispatchUID:` is `v24@0:8(?={?=ii}Q)16` (the
                // `DispatchUid` union), `-setSolid:` is `v20@0:8B16` (BOOL).
                unsafe {
                    let () = msg_send![&*object, setDispatchUID: DispatchUid(uid)];
                    let () = msg_send![&*object, setSolid: false];
                }
                objects.push(object);
            }
            let batch: Option<Retained<AnyObject>> = unsafe {
                let a: Allocated<AnyObject> = msg_send![batch_class, alloc];
                msg_send![a, init]
            };
            let batch = batch.ok_or(SessionError::FetchObjectInit {
                name: "GTReplayRequestBatch",
            })?;
            let array = NSArray::from_retained_slice(&objects);
            let () = unsafe { msg_send![&*batch, setRequests: &*array] };

            let mark = self.observer.error_count();
            let outcome: Outcome = Arc::new(Mutex::new(None));
            let sink = Arc::clone(&outcome);
            let handler = RcBlock::new(move |response: *mut AnyObject| {
                let result = autoreleasepool(|_| unsafe { read_response(response) });
                let mut slot = match sink.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                if slot.is_none() {
                    *slot = Some(result);
                }
            });
            let () = unsafe { msg_send![&*batch, setCompletionHandler: &*handler] };
            let token: Option<Retained<AnyObject>> =
                unsafe { msg_send![self.service, fetch: &*batch] };
            let token = token.ok_or(SessionError::NoToken)?;
            let result = wait_for(&outcome, timeout);
            std::mem::forget(batch);
            std::mem::forget(token);
            if let Some(message) = self.observer.first_error_since(mark) {
                if std::env::var_os("PROBE_TOLERATE_REPLAYER_ERRORS").is_some_and(|v| v == "1") {
                    eprintln!("fetch_wireframe: replayer error (tolerated): {message}");
                } else {
                    return Err(SessionError::ReplayerError { message });
                }
            }
            result
        })
    }
}

/// Builds the `GTReplayRequestBatch`. The request objects are held by the
/// batch's `requests` array, which is a retaining property, so only the batch
/// itself needs an owner.
fn build_batch(requests: &[FetchRequest]) -> Result<Retained<AnyObject>, SessionError> {
    let fetch_class =
        AnyClass::get(c"GTReplayFetchTexture").ok_or(SessionError::FetchClassMissing {
            name: "GTReplayFetchTexture",
        })?;
    let batch_class =
        AnyClass::get(c"GTReplayRequestBatch").ok_or(SessionError::FetchClassMissing {
            name: "GTReplayRequestBatch",
        })?;

    let mut objects: Vec<Retained<AnyObject>> = Vec::with_capacity(requests.len());
    for request in requests {
        // SAFETY: NSObject's designated initialiser on a fresh allocation of a
        // class that declares no other.
        let object: Option<Retained<AnyObject>> = unsafe {
            let allocated: Allocated<AnyObject> = msg_send![fetch_class, alloc];
            msg_send![allocated, init]
        };
        let object = object.ok_or(SessionError::FetchObjectInit {
            name: "GTReplayFetchTexture",
        })?;
        let size = GTSize {
            width: request.width,
            height: request.height,
            depth: 1,
        };
        let region = GTRegion {
            origin: GTPoint3D { x: 0, y: 0, z: 0 },
            size,
        };
        // SAFETY: each setter's encoding was read off the live class:
        // `-setStreamRef:` `v24@0:8Q16`, `-setSize:` `{GTSize=QQQ}`,
        // `-setRegion:` `{GTRegion=...}`, `-setPlane:` and `-setDepth:`
        // `v20@0:8I16` - i.e. `u32`, not `u64`.
        unsafe {
            let () = msg_send![&*object, setStreamRef: request.stream_ref];
            let () = msg_send![&*object, setSize: size];
            let () = msg_send![&*object, setRegion: region];
            let () = msg_send![&*object, setPlane: request.plane];
            // Always send depth = 1, never the region's depth: `-depth` is the
            // slice count, and a 2D texture has one. depth = 0 returns nothing
            // from the replayer - the prior project's most expensive wrong
            // conclusion.
            let () = msg_send![&*object, setDepth: 1u32];
        }
        objects.push(object);
    }

    // SAFETY: as above.
    let batch: Option<Retained<AnyObject>> = unsafe {
        let allocated: Allocated<AnyObject> = msg_send![batch_class, alloc];
        msg_send![allocated, init]
    };
    let batch = batch.ok_or(SessionError::FetchObjectInit {
        name: "GTReplayRequestBatch",
    })?;
    let array = NSArray::from_retained_slice(&objects);
    // SAFETY: `-setRequests:` is `v24@0:8@16`, and `requests` is declared
    // `@"NSArray"`.
    let () = unsafe { msg_send![&*batch, setRequests: &*array] };
    Ok(batch)
}

/// Waits for the completion, pumping the run loop.
///
/// Why pump at all: `-[GTReplayRequestToken waitUntilCompleted]` is the obvious
/// call and it blocks the calling thread forever at 0% CPU (measured), so it
/// cannot be used. Pumping is what the working ObjC probe did, and what is
/// known to work.
///
/// **The mechanism is NOT established.** The completion fires on a framework
/// thread, and a run loop pumped on a non-main thread does not drain the main
/// dispatch queue - libtest runs each test on a spawned thread, so during the
/// oracle run the main queue provably is not serviced and the fetch succeeds
/// anyway. The pump is therefore very likely not what delivers the response.
/// What it reliably provides is a bounded wait that yields the CPU. Do not
/// remove it on the theory that it does nothing until you have established
/// what does deliver the completion.
fn wait_for(outcome: &Outcome, timeout: Duration) -> Result<Vec<u8>, SessionError> {
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
            return Err(SessionError::FetchTimedOut {
                seconds: timeout.as_secs(),
            });
        }
        pump_run_loop();
    }
}

/// One turn of this thread's run loop in the default mode.
fn pump_run_loop() {
    let run_loop = NSRunLoop::currentRunLoop();
    let until = NSDate::dateWithTimeIntervalSinceNow(PUMP_SECONDS);
    // SAFETY: reading a constant the framework exports.
    let mode = unsafe { NSDefaultRunLoopMode };
    if !run_loop.runMode_beforeDate(mode, &until) {
        // The loop had no input source to run, so it returned at once rather
        // than blocking until `until`. Without this the wait would spin a core
        // flat while the completion runs on another thread.
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Reads the one argument the completion block is handed. Everything it returns
/// is owned Rust data, so no ObjC handle escapes the framework's thread.
///
/// # Safety
///
/// `response` must be null or a live object.
unsafe fn read_response(response: *mut AnyObject) -> Result<Vec<u8>, SessionError> {
    if response.is_null() {
        return Err(SessionError::NoResponse);
    }
    // Every selector this function sends must appear here. An ObjC
    // `doesNotRecognizeSelector:` raise unwinding through these frames is
    // undefined behaviour, so "responds to it" is the whole basis for sending
    // anything at all, and a guard that covers only some of the selectors sent
    // is not a guard.
    // SAFETY: `-respondsToSelector:` is NSObject protocol, so any object
    // answers it.
    let answers: bool = unsafe {
        let data: bool = msg_send![response, respondsToSelector: sel!(data)];
        let error: bool = msg_send![response, respondsToSelector: sel!(error)];
        data && error
    };
    if !answers {
        return Err(SessionError::NoResponse);
    }
    // SAFETY: `-error` is `@16@0:8`, declared `@"NSError"`.
    let error: Option<Retained<NSError>> = unsafe { msg_send![response, error] };
    if let Some(error) = error {
        return Err(SessionError::FetchFailed {
            message: truncate(&error.localizedDescription().to_string()),
        });
    }
    // SAFETY: `-data` is `@16@0:8`, declared `@"NSData"`.
    let data: Option<Retained<NSData>> = unsafe { msg_send![response, data] };
    match data {
        Some(data) => Ok(data.to_vec()),
        None => Err(SessionError::NoData),
    }
}

impl Session {
    /// The replay controller, read from field 1 of the client struct.
    ///
    /// See [`ClientBuffer::controller`] for the derivation. Null until
    /// `GTMTLReplayClient_init` has run, which `open` guarantees before any
    /// `Session` exists.
    pub fn controller_in_client(&self) -> *mut GTMTLReplayController {
        // SAFETY: `self._client` is the live `ClientBuffer` allocated in `open`
        // and never freed (see the field doc), so a shared reference to it is
        // valid here.
        unsafe { (*self._client).controller() }
    }

    /// Calls `GTMTLReplayClient_preferDevice(client)` on this session's client.
    ///
    /// Must be called after `open` (which does init + `load:`), never on a
    /// fresh client: the function dereferences client fields that `load:`
    /// populates. The device to prefer is resolved internally from
    /// `MTLREPLAYER_OVERRIDE_DEVICE_REGISTRY_ID`; there is no device argument.
    /// Probe-only, for the behaviour audit (dossier 06).
    pub fn prefer_device(&self) {
        // SAFETY: `self._client` is the live, loaded client from `open`; the
        // function takes exactly one argument, the client, and returns void
        // (established, dossier 06).
        unsafe {
            let client = (*self._client).as_client_ptr();
            gputools_replay_sys::ffi::GTMTLReplayClient_preferDevice(client);
        }
    }

    /// Address of the client struct, for out-of-process inspection
    /// (`sample`/`vmmap`). Diagnostics only.
    pub fn client_addr(&self) -> usize {
        self._client as usize
    }

    /// The live `GTMTLReplayService` object, for runtime introspection of the
    /// loaded object graph (finding the resource object map). Probe-only.
    pub fn service(&self) -> *mut AnyObject {
        self.service
    }

    /// The controller's current command index, at byte offset `0x5820`.
    ///
    /// Established in `docs/findings/01-playback.md`: `playTo`'s exit test at
    /// `0x24f84db84` loads this field and compares it against the target, and
    /// its back-edge increments and stores it. Reading it before and after a
    /// playback call is what distinguishes real forward progress from the
    /// documented no-op (`currentIndex >= target` returns immediately).
    pub fn command_index(&self) -> u32 {
        // SAFETY: `controller_in_client` is the controller the framework
        // itself recorded in the client struct, so it is a live controller of
        // at least `0x5824` bytes (the framework's own code loads and stores
        // this field). The read is aligned (0x5820 is 4-aligned) and of the
        // `w`-register width the disassembly uses.
        unsafe {
            self.controller_in_client()
                .cast::<u8>()
                .add(0x5820)
                .cast::<u32>()
                .read()
        }
    }
}
