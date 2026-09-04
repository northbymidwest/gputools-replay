//! Session bootstrap, the process-global one-per-process guard, the ObjC
//! error observer, and playback controls.
//!
//! This module drives Apple's private GPUToolsReplay framework in-process.
//! Ported faithfully from `probes/src/session.rs`, the validated reference:
//! every `// SAFETY:` justification and the exact bootstrap ordering are
//! carried over unchanged. The one addition is [`crate::ReplayerConfig`],
//! whose env vars are applied before the framework is initialised.

use crate::SessionError;
use crate::config::ReplayerConfig;
use crate::util::truncate;
use gputools_replay_sys::client::{
    AprPool, ClientBuffer, GTMTLReplayClient, GTMTLReplayController,
};
use gputools_replay_sys::ffi::{
    GTMTLReplayClient_init, GTMTLReplayController_init, GTMTLReplayController_playAll,
    GTMTLReplayController_playTo, GTMTLReplayController_rewind,
    GTMTLReplayErrorHandling_initWithObserver, apr_initialize, apr_pool_create_ex,
};
use gputools_replay_sys::replay::GTMTLReplayService;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{NSError, NSString, NSURL};
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set once the process-global bootstrap has been attempted. Never cleared:
/// a failed attempt has already touched the global state, so a retry is no
/// safer than a second success would be.
static BOOTSTRAPPED: AtomicBool = AtomicBool::new(false);

/// Entries every known .gputrace bundle carries.
const REQUIRED_ENTRIES: [&str; 2] = ["index", "metadata"];

/// Rejects anything `load:` would crash on. Run before any global state is
/// touched, so a rejected path leaves the process able to open a session.
fn check_bundle_shape(bundle: &Path) -> Result<(), SessionError> {
    if !bundle.exists() {
        return Err(SessionError::BadBundle(format!(
            "no capture bundle at {}",
            bundle.display()
        )));
    }
    for entry in REQUIRED_ENTRIES {
        if !bundle.join(entry).is_file() {
            return Err(SessionError::BadBundle(format!(
                "{} is not a capture bundle: it has no {entry} file",
                bundle.display()
            )));
        }
    }
    Ok(())
}

struct ObserverIvars {
    /// The replayer reports from queues of its own, so this is locked.
    errors: Mutex<Vec<String>>,
}

define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - ErrorObserver does not implement Drop.
    #[unsafe(super(NSObject))]
    #[ivars = ObserverIvars]
    struct ErrorObserver;

    unsafe impl NSObjectProtocol for ErrorObserver {}

    impl ErrorObserver {
        /// The one selector `GTMTLReplayErrorHandling` sends to its observer.
        /// The framework ALWAYS passes an `NSError` here (MEASURED: a
        /// triggered replayer error arrives as class `NSError`; and every
        /// error-carrying block in the extracted framework binary is typed
        /// `@"NSError"`). But `-notifyError:` itself is declared with a bare
        /// `id` (encoding `v24@0:8@16`, not `@"NSError"`), so the type system
        /// cannot guarantee it - hence the `Option<&AnyObject>` signature and
        /// the isKindOfClass-checked `downcast_ref::<NSError>()` before using
        /// NSError semantics, with a safe `-description` fallback (via an
        /// NSObject downcast) should the framework ever pass something other
        /// than an `NSError`.
        #[unsafe(method(notifyError:))]
        fn notify_error(&self, error: Option<&AnyObject>) {
            let message = match error {
                // isKindOfClass check: an actual NSError yields its
                // human-readable `-localizedDescription`, the correct
                // error-message accessor (not the verbose `-description`,
                // "Error Domain=... Code=... UserInfo=...").
                Some(e) => match e.downcast_ref::<NSError>() {
                    Some(err) => truncate(&err.localizedDescription().to_string()),
                    // Not an NSError (provably unreachable upstream per the
                    // selector note above, but the `id` signature cannot prove
                    // it). Downcast to NSObject so `-description` is the safe
                    // `NSObjectProtocol` method, not a raw send.
                    None => match e.downcast_ref::<NSObject>() {
                        Some(obj) => {
                            // SAFETY: `-description` always returns an
                            // `NSString`; objc2's `NSObjectProtocol::description`
                            // docs bless this exact cast as always safe.
                            let described: Retained<NSString> =
                                unsafe { Retained::cast_unchecked(obj.description()) };
                            truncate(&described.to_string())
                        }
                        // Not even an NSObject (e.g. an NSProxy root): nothing
                        // safe to message.
                        None => "(no description)".to_owned(),
                    },
                },
                None => "(nil)".to_owned(),
            };
            self.record(message);
        }
    }
);

impl ErrorObserver {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(ObserverIvars {
            errors: Mutex::new(Vec::new()),
        });
        // SAFETY: NSObject's designated initialiser, on a fresh allocation.
        unsafe { msg_send![super(this), init] }
    }

    fn record(&self, message: String) {
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
    fn first_error(&self) -> Option<String> {
        self.first_error_since(0)
    }

    /// How many errors have been reported so far. Taken before an operation so
    /// that `first_error_since` can attribute an error to it.
    fn error_count(&self) -> usize {
        self.errors().len()
    }

    /// The first error reported after `mark`. A session outlives many fetches
    /// and this observer never forgets, so asking for the first error outright
    /// would blame every later fetch for the first one's failure.
    fn first_error_since(&self, mark: usize) -> Option<String> {
        self.errors().get(mark..)?.first().cloned()
    }

    fn errors(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        match self.ivars().errors.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// A live handle to Apple's private GPUToolsReplay framework, driving one
/// capture bundle in-process.
///
/// Only one `Session` can ever exist in a process: the framework's own state
/// (one APR pool, one controller, one error observer) is process-global, and
/// [`Session::open`] enforces that with an atomic one-shot guard, returning
/// [`SessionError::AlreadyOpen`] on a second call. Nothing a `Session` owns is
/// ever freed; see the field doc comments for why.
///
/// `!Send` and `!Sync`: the framework's objects are not documented as
/// thread-safe, so a `Session` is pinned to the thread that opened it.
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
    service: *mut GTMTLReplayService,
    /// Raw pointers already make this type `!Send`/`!Sync`; this makes that
    /// invariant explicit and robust against a future field change.
    _not_send_sync: PhantomData<*const ()>,
}

impl Session {
    /// Apply `config`'s `MTLREPLAYER_*` replayer environment variables.
    /// Call once, early in `main`, alongside the
    /// `MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX` setup the framework
    /// already requires, and BEFORE [`Session::open`].
    ///
    /// # Safety
    /// Writes environment variables via [`std::env::set_var`], which races
    /// with concurrent environment reads. Call only while the process is
    /// single-threaded (before it spawns any thread that reads the
    /// environment). This is the same precondition, and the same reason,
    /// as `gputools_replay_sys::env`'s unlock-env setup.
    pub unsafe fn configure_env(config: &ReplayerConfig) {
        // SAFETY: the caller upholds the single-threaded precondition in
        // this fn's `# Safety` contract.
        unsafe { config.apply_env() }
    }

    /// Opens a session against `bundle`.
    ///
    /// Only one `Session` can ever exist in a process: the framework's own
    /// state is process-global, and `open` enforces that with an atomic
    /// one-shot guard, returning [`SessionError::AlreadyOpen`] on a second
    /// call.
    ///
    /// To apply non-default replayer configuration, call
    /// [`Session::configure_env`] (`unsafe`) before this.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::BadBundle`] if `bundle` does not have the
    /// shape `load:` requires (checked first, so a rejected path leaves the
    /// process able to open a session afterwards), [`SessionError::UnlockEnv`]
    /// if `MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX` is not set to `"0"`,
    /// [`SessionError::AlreadyOpen`] if a session has already been opened in
    /// this process, [`SessionError::Apr`] if APR bootstrap fails, and
    /// [`SessionError::Replayer`] or [`SessionError::LoadFailed`] if the
    /// framework itself refuses the capture.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use gputools_replay::{Session, config::ReplayerConfig, request::TextureRequest};
    /// use std::path::Path;
    /// use std::time::Duration;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = ReplayerConfig::default();
    /// // SAFETY: single-threaded, before open.
    /// unsafe { Session::configure_env(&config) };
    /// let session = Session::open(Path::new("captures/small.gputrace"))?;
    ///
    /// let requests = [TextureRequest::natural(0)];
    /// let reply = session.fetch_textures(&requests, Duration::from_secs(60))?;
    /// if let Some(record) = reply.records().first() {
    ///     println!("stream_ref={} width={}", record.stream_ref, record.width);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn open(bundle: &Path) -> Result<Self, SessionError> {
        // Checked before anything global is touched, so that these failures
        // leave the process able to open a session afterwards.
        check_bundle_shape(bundle)?;
        let path = bundle.to_str().ok_or_else(|| {
            SessionError::BadBundle(format!(
                "bundle path {} is not valid UTF-8",
                bundle.display()
            ))
        })?;
        // Also checked before the one-shot guard below, so that a caller who
        // forgot the variable can set it and try again.
        gputools_replay_sys::env::unlock_env_ok()
            .map_err(|e| SessionError::UnlockEnv(e.to_string()))?;
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
                return Err(SessionError::Apr { code: rc });
            }
            let mut pool: *mut AprPool = std::ptr::null_mut();
            let rc = apr_pool_create_ex(
                &mut pool,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if rc != 0 || pool.is_null() {
                return Err(SessionError::Apr { code: rc });
            }
            // Return value deliberately discarded: this initialises global
            // config from the environment and does NOT vend a controller.
            // Apple's own service discards it too.
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

        // The class is exported by the framework, which is linked, so it is
        // loaded before main. The `AnyClass::get` guard stays BEFORE
        // `alloc()`, which panics on an unregistered class: a miss means the
        // framework did not load, and that must surface as
        // `SessionError::Replayer`, not a panic (same pattern as
        // `objc::new_request`). `-initWithContext:` takes the client pointer.
        if AnyClass::get(c"GTMTLReplayService").is_none() {
            return Err(SessionError::Replayer {
                message:
                    "the GTMTLReplayService class is not registered; GPUToolsReplay did not load"
                        .to_owned(),
            });
        }
        let service: *mut GTMTLReplayService =
            match GTMTLReplayService::init_with_context(GTMTLReplayService::alloc(), client) {
                // Into a raw pointer, i.e. leaked: releasing a replay service
                // is not an operation this project has established as safe.
                // Stored as its own typed pointer, which `fetch.rs` reborrows.
                Some(service) => Retained::into_raw(service),
                None => std::ptr::null_mut(),
            };
        if service.is_null() {
            return Err(SessionError::Replayer {
                message: "GTMTLReplayService could not be constructed".to_owned(),
            });
        }

        let loaded = load_bundle(service, path)?;
        // `load:` returning YES only reports that its dispatched block ran, so
        // the observer is the authority on whether the replay actually worked.
        if let Some(message) = observer.first_error() {
            return Err(SessionError::Replayer { message });
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
            _not_send_sync: PhantomData,
        })
    }

    /// Drives playback to the end of the command stream.
    ///
    /// # Behavior
    ///
    /// **Unverified.** The signature (one argument, the controller) is
    /// established from disassembly, so the call is safe with respect to
    /// arity and argument types, but the behavior of the call against a live
    /// replayer is not independently established here.
    pub fn play_all(&self) {
        // SAFETY: `self.controller_in_client()` came from
        // `GTMTLReplayController_init` on this session's own pool in `open`
        // and is valid for the session's life (see the field doc comment).
        // `GTMTLReplayController_playAll` takes exactly one argument, the
        // controller; the BEHAVIOR of the call is not established.
        unsafe { GTMTLReplayController_playAll(self.controller_in_client()) };
    }

    /// Drives playback forward from the controller's current command index up
    /// to (not including) `index`. Never rewinds: if the current index is
    /// already `>= index` this is a no-op (established control flow).
    ///
    /// # Behavior
    ///
    /// **Unverified**, for the same reason as [`Session::play_all`]: the
    /// signature is established, the effect on a live replayer is not.
    pub fn play_to(&self, index: u32) {
        // SAFETY: as `play_all`. `GTMTLReplayController_playTo` takes exactly
        // two arguments, the controller and a `u32` target command index; the
        // BEHAVIOR of the call is not established.
        unsafe { GTMTLReplayController_playTo(self.controller_in_client(), index) };
    }

    /// Rewinds the controller: tears down, then restores initial state
    /// (inferred from control flow).
    ///
    /// # Behavior
    ///
    /// **Unverified**, for the same reason as [`Session::play_all`]: the
    /// signature is established, the effect on a live replayer is not.
    pub fn rewind(&self) {
        // SAFETY: as `play_all`. `GTMTLReplayController_rewind` takes exactly
        // one argument, the controller; the BEHAVIOR of the call is not
        // established.
        unsafe { GTMTLReplayController_rewind(self.controller_in_client()) };
    }

    /// The receiver of `fetch:`. `pub(crate)` for the fetch layer built on top
    /// of this module.
    pub(crate) fn service(&self) -> *mut GTMTLReplayService {
        self.service
    }

    /// How many errors the error observer has recorded so far. `pub(crate)`
    /// for the fetch layer, which marks this before a fetch so it can
    /// attribute errors reported afterwards to that fetch specifically.
    pub(crate) fn observer_error_count(&self) -> usize {
        self.observer.error_count()
    }

    /// The first error the observer recorded after `mark`. `pub(crate)` for
    /// the fetch layer.
    pub(crate) fn observer_first_error_since(&self, mark: usize) -> Option<String> {
        self.observer.first_error_since(mark)
    }

    /// A cheap, `'static`-capturable handle to this session's observer
    /// errors, for the fetch layer's `'static` completion-handler closure
    /// (`fetch.rs`), which cannot borrow `&Session` across the async
    /// boundary to a framework thread. Clones `self.observer` (a
    /// `Retained<ErrorObserver>`: ObjC retain/release is atomic, and
    /// `ErrorObserver`'s error list is already behind its own `Mutex`), so
    /// calling the returned closure from the framework's completion thread,
    /// well after `Session::open` returns, is sound even though the
    /// closure is not `Send` in the type system.
    pub(crate) fn observer_checker(&self) -> impl Fn(usize) -> Option<String> + 'static {
        let observer = self.observer.clone();
        move |mark: usize| observer.first_error_since(mark)
    }
}

/// Sends `-load:error:`, which takes an **NSURL**: passing a
/// `GTReplayLoadRequest` raises `-[GTReplayLoadRequest scheme]: unrecognized
/// selector`. Measured, not inferred.
fn load_bundle(service: *mut GTMTLReplayService, path: &str) -> Result<bool, SessionError> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let mut error: Option<Retained<NSError>> = None;
    // SAFETY: `service` is a live `GTMTLReplayService` for the whole session
    // (leaked into a raw pointer in `Session::open`), so reborrowing it for
    // this synchronous call is sound. `-load:error:` is the typed `objc::`
    // method (encoding `B32@0:8@16^@24`).
    let ok = unsafe { (*service).load(&url, Some(&mut error)) };
    match error {
        Some(e) => Err(SessionError::Replayer {
            message: truncate(&e.localizedDescription().to_string()),
        }),
        None => Ok(ok),
    }
}

/// Byte offset of the controller's current command index within
/// `GTMTLReplayController`. Established: `playTo`'s exit test at
/// `0x24f84db84` loads this field and compares it against the target, and its
/// back-edge increments and stores it.
const COMMAND_INDEX_OFFSET: usize = 0x5820;

impl Session {
    /// The replay controller, read from field 1 of the client struct.
    ///
    /// See [`ClientBuffer::controller`] for the derivation. Null until
    /// `GTMTLReplayClient_init` has run, which `open` guarantees before any
    /// `Session` exists.
    fn controller_in_client(&self) -> *mut GTMTLReplayController {
        // SAFETY: `self._client` is the live `ClientBuffer` allocated in `open`
        // and never freed (see the field doc), so a shared reference to it is
        // valid here.
        unsafe { (*self._client).controller() }
    }

    /// The controller's current command index, at byte offset
    /// `COMMAND_INDEX_OFFSET`.
    ///
    /// Reading it before and after a playback call is what distinguishes real
    /// forward progress from the documented no-op (`currentIndex >= target`
    /// returns immediately).
    pub fn command_index(&self) -> u32 {
        // SAFETY: `controller_in_client` is the controller the framework
        // itself recorded in the client struct, so it is a live controller of
        // at least `COMMAND_INDEX_OFFSET + 4` bytes (the framework's own code
        // loads and stores this field). The read is aligned
        // (`COMMAND_INDEX_OFFSET` is 4-aligned) and of the `w`-register width
        // the disassembly uses.
        unsafe {
            self.controller_in_client()
                .cast::<u8>()
                .add(COMMAND_INDEX_OFFSET)
                .cast::<u32>()
                .read()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_bundle_is_rejected_without_touching_globals() {
        let err = Session::open(Path::new("/nonexistent.gputrace"));
        assert!(matches!(
            err,
            Err(SessionError::BadBundle(_) | SessionError::UnlockEnv(_))
        ));
    }
}
