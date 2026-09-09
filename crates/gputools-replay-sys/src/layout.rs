//! Non-FFI memory-layout knowledge: the Rust-side backing buffer for the
//! replay client, and the MEASURED byte offsets by which the opaque foreign
//! structs are navigated. None of this is an `extern` binding - it is pointer
//! arithmetic over the FFI types defined in [`crate::client`] and
//! [`crate::replay`]. The derivations are recorded in `docs/HANDOFF.md`.

use crate::client::{GTMTLReplayClient, GTMTLReplayController};
use objc2::runtime::AnyObject;

/// `sizeof(struct GTMTLReplayClient)` = 312 bytes, 0x138. Derived, not chosen.
///
/// HOW IT WAS DERIVED: [`crate::client::CLIENT_ENCODING`] is the struct's
/// complete layout, and the framework itself is the source of it.
/// `NSGetSizeAndAlignment` cannot parse it - it raises `NSInvalidArgumentException:
/// unsupported type encoding spec 'b'` on the `b1b1b1b1b28` bitfield run - so the
/// layout was taken from the compiler that emitted the encoding instead.
/// `spike/probeI.m` transcribes the encoding into a C struct, asserts that
/// `@encode()` of the transcription is byte-for-byte identical to the string the
/// runtime reports, and prints `sizeof` = 312 and `_Alignof` = 8.
///
/// WHAT THE ENCODING DOES NOT RECORD, because "identical string implies
/// identical layout" is not free:
///
/// - A bitfield's *declared storage type*: `b28` says a width, not what it was
///   declared in. **Checked and harmless.** `probeI.m` rebuilds the whole
///   struct with the run declared `unsigned int`, `unsigned long long`, `int`
///   and `long`; all four give 312 and 8, with the containing field 24 bytes
///   at 0x40 and `f8` at 0x128 in every case.
/// - An `aligned` or `packed` attribute on a member. **This is the one thing
///   still assumed.** An over-aligned member could in principle enlarge the
///   real struct while still encoding to a byte-identical string, and no
///   inspection of the encoding can rule that out. What constrains it is the
///   measurement below: the framework's writes land at exactly the offsets
///   this layout predicts, all the way out to `f8` at 0x128. Any added padding
///   would have to fall after that and leave every earlier offset untouched.
///
/// Foundation was asked as well, and agrees on 312 and 8 once the bitfield run
/// is rewritten as the `I` its parser accepts. Read that as corroboration of
/// the transcription, *not* as an independent check: rewriting the run as `I`
/// hard-codes the assumption that it occupies one 32-bit unit. It is a second
/// parser, not a second opinion on anything the encoding left open.
///
/// HOW IT WAS CHECKED: `spike/probeJ.m` hands the framework a 64 KiB buffer
/// with [0, 312) zeroed - so the callee sees precisely what the reference's
/// `calloc` gave it - and everything above 312 filled with a
/// position-dependent poison, then runs the whole lifecycle against a real
/// capture: `GTMTLReplayClient_init`, `initWithContext:`, `load:`, and one
/// completed `fetch:`. Every write landed in [0x00, 0x12c]. Not one byte at or
/// after 0x138 changed, so nothing wrote past the derived size even by storing
/// a zero. Strictly, that measurement bounds the size below at 0x12d, and at
/// 0x130 if the byte at 0x12c is, as the derived layout says, part of the
/// 8-byte field spanning [0x128, 0x130). 312 clears either. The remaining
/// 8 bytes are one `@` field the framework left untouched on that run, which
/// is exactly the kind of field a write-extent measurement cannot see.
///
/// The probe's touched-slot map does more than bound the end, though: it shows
/// 0x110, 0x118, 0x120 and 0x128 written and **0x130 not written**. A layout
/// shifted by +8 anywhere before the end - the nearest alternative, and what a
/// missed padding byte would produce - would have moved that last write into
/// 0x130. It did not. That rules the shifted layout out positively, rather
/// than merely failing to contradict it.
///
/// Note that the disassembled extent recorded in `docs/spike-findings.md`,
/// `[x0+0x00]..[x0+0x110]`, understates what the initialiser really writes,
/// which is why the size comes from the type rather than from the trace.
pub const CLIENT_BUF_LEN: usize = 312;

/// Backing store for the `GTMTLReplayClient` the framework initialises: one
/// instance of the struct, no more and no less.
///
/// Aligned to 16 while the type itself requires only 8 (its widest member is a
/// pointer). That is over-alignment, which cannot violate an 8-byte
/// requirement, and it is what every real caller supplies: Apple's own
/// GPUToolsReplayService hands this function a `malloc`ed client, and so did
/// `probeF.m`, the working reference. 16 is therefore the alignment the
/// framework is actually exercised at. `vec![0u8; N]` would give `align == 1`
/// and survive only by accident of the global allocator forwarding small
/// alignments to `malloc`.
///
/// `repr(align(16))` rounds the wrapper's own size up to 320: 8 bytes of tail
/// padding past the 312 the struct occupies. Slack, never given a meaning,
/// the framework is handed a pointer to a `GTMTLReplayClient`, and 312 bytes
/// is what one of those is.
#[repr(C, align(16))]
pub struct ClientBuffer(pub [u8; CLIENT_BUF_LEN]);

impl ClientBuffer {
    pub fn new_zeroed() -> Self {
        Self([0u8; CLIENT_BUF_LEN])
    }

    /// The pointer to hand `GTMTLReplayClient_init` and `initWithContext:`.
    pub fn as_client_ptr(&mut self) -> *mut GTMTLReplayClient {
        std::ptr::from_mut(self).cast()
    }

    /// The replay controller, read out of field 1 of the initialised client.
    ///
    /// [`crate::client::CLIENT_ENCODING`] opens with
    ///
    /// ```text
    /// {GTMTLReplayClient=^{apr_pool_t}^{GTMTLReplayController}Q...}
    /// ```
    ///
    /// so field 0 is the APR pool and field 1 is the controller. Both are
    /// pointers, so the controller sits at byte offset [`CONTROLLER_OFFSET`].
    /// The encoding is the runtime's own, and the regression test in
    /// [`crate::client`] fails the build if it ever changes.
    ///
    /// This is the ONLY established way to obtain the controller.
    /// `GTMTLReplayController_init`'s return value is NOT it: that function is
    /// a global configuration initialiser and Apple's own replay service
    /// discards what it returns. Passing that value to `playAll`/`playTo`
    /// faults on the first controller dereference; see
    /// `docs/findings/01-playback.md`.
    ///
    /// Returns null before `GTMTLReplayClient_init` has run (the buffer starts
    /// zeroed), so callers must treat null as "not initialised yet".
    pub fn controller(&self) -> *mut GTMTLReplayController {
        // SAFETY: `CONTROLLER_OFFSET + size_of::<*mut _>() <= CLIENT_BUF_LEN`,
        // so the read is in bounds of `self.0`. The field is 8-aligned within
        // a 16-aligned buffer at offset 8, and the encoding types it as a
        // `^{GTMTLReplayController}`, so it is a valid aligned pointer read.
        unsafe {
            std::ptr::from_ref(self)
                .cast::<u8>()
                .add(CONTROLLER_OFFSET)
                .cast::<*mut GTMTLReplayController>()
                .read()
        }
    }
}

/// Byte offset of the controller within `GTMTLReplayClient`: field 1, straight
/// after the `^{apr_pool_t}` at field 0. See [`ClientBuffer::controller`].
pub const CONTROLLER_OFFSET: usize = 8;

const _: () =
    assert!(CONTROLLER_OFFSET + size_of::<*mut GTMTLReplayController>() <= CLIENT_BUF_LEN);

/// Byte offset of the [`crate::replay::GTMTLReplayObjectMap`] pointer within the
/// controller struct. MEASURED, not derived: `GTMTLReplayController` is an
/// opaque struct with no field encoding to re-derive from (unlike
/// [`CONTROLLER_OFFSET`], which comes from the client encoding), so this was
/// found live by `probes/src/bin/objectmap.rs` (2026-09-04) - a
/// `malloc_size`-guarded heap scan located the map at `controller + 0x8`, stable
/// across two captures. That probe is its living regression check.
pub const OBJECT_MAP_OFFSET: usize = 0x8;

/// The object hanging off a loaded controller at [`OBJECT_MAP_OFFSET`], returned
/// as an unverified `*mut AnyObject`: the offset is MEASURED, so the type is not
/// yet proven. Confirm it with a class-checked `downcast_ref::<GTMTLReplayObjectMap>`
/// before use - the honest result of reading a raw offset, not an asserted type.
/// The pointer is borrowed (not retained) and valid only while the controller
/// (and its session) lives.
///
/// # Safety
/// `controller` must be a live, loaded `GTMTLReplayController` (post-`load:`, as
/// [`ClientBuffer::controller`] returns after `open`).
pub unsafe fn controller_object_map(controller: *mut GTMTLReplayController) -> *mut AnyObject {
    // SAFETY: the caller guarantees `controller` is a live controller, which is
    // far larger than OBJECT_MAP_OFFSET + a pointer (its command index alone is
    // at 0x5820), so the pointer-sized read at 0x8 is in bounds.
    unsafe { ((controller as usize + OBJECT_MAP_OFFSET) as *const *mut AnyObject).read() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, size_of};

    /// The buffer is exactly one struct, 16-byte aligned (over-satisfying
    /// the type's own 8), and `repr(align(16))` pads the wrapper to 320.
    #[test]
    fn the_client_buffer_is_one_struct_and_is_sixteen_byte_aligned() {
        assert_eq!(CLIENT_BUF_LEN, 312);
        assert_eq!(align_of::<ClientBuffer>(), 16);
        assert_eq!(size_of::<ClientBuffer>(), 320);
        let b = Box::new(ClientBuffer::new_zeroed());
        assert_eq!(std::ptr::from_ref(&*b).addr() % 16, 0);
    }
}
