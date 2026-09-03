//! Established C signatures. Resolved by the linker against the dyld shared
//! cache (see build.rs): no dlopen, no dlsym. Each signature was established
//! from a real caller or a callee prologue, never guessed: wrong arity here is
//! memory corruption, not an error message. Provenance: HANDOFF 2.2.
//!
//! Unestablished exports are deliberately NOT declared here; see the
//! `inventory` module. A probe establishing a new surface declares its
//! signature-under-test in the probe binary, and only a confirmed signature
//! graduates into this block.

use crate::client::{AprPool, GTMTLReplayClient, GTMTLReplayController};
use objc2::runtime::AnyObject;
use std::ffi::{c_int, c_void};

unsafe extern "C" {
    /// Standard APR. Returns 0 on success.
    pub fn apr_initialize() -> c_int;
    /// Standard APR. The last two arguments are `apr_abortfunc_t` and
    /// `apr_allocator_t*`; both are passed NULL by every observed caller and
    /// neither type was established, so neither is given one (HANDOFF 2.2).
    pub fn apr_pool_create_ex(
        pool: *mut *mut AprPool,
        parent: *mut AprPool,
        abort_fn: *mut c_void,
        allocator: *mut c_void,
    ) -> c_int;
    /// One argument, the pool. Read off GPUToolsReplayService's own binary,
    /// which does apr_initialize -> apr_pool_create_ex -> this.
    ///
    /// Despite the name this does NOT allocate or return a controller. It is a
    /// global configuration initialiser: it points at the framework's `_GT_ENV`
    /// global and fills roughly fifty of its bits from `MTLREPLAYER_*`
    /// environment variables via `_GetEnvDefault`. GPUToolsReplayService calls
    /// it exactly as we do and then DISCARDS the return value.
    ///
    /// The return is therefore declared opaque and must not be used. Treating
    /// it as the controller is what produced the "playback never completes"
    /// dead end: the leftover value points into the framework's read-only
    /// `__AUTH_CONST` segment, and `playTo` faults on the first dereference.
    /// Get the controller from [`crate::client::ClientBuffer::controller`]
    /// instead. See `docs/findings/01-playback.md`.
    pub fn GTMTLReplayController_init(pool: *mut AprPool) -> *mut c_void;
    /// x0 is a caller-allocated out-buffer of `client::CLIENT_BUF_LEN` bytes
    /// (see that constant's derivation); x1 lands in struct field 0, typed
    /// `^{apr_pool_t}`. Exactly two arguments: the prologue never reads x2/x3/x4.
    pub fn GTMTLReplayClient_init(out: *mut GTMTLReplayClient, pool: *mut AprPool);
    /// Selects which real device the replay binds. Exactly ONE argument, the
    /// client/self pointer (function @ 0x24f7fe0b4; `mov x23,x0` is the only
    /// incoming-argument use). It dereferences client fields `[client]`,
    /// `[client+0x30]`, `[client+0x60]`, which must already be populated - so it
    /// must be called AFTER `-load:error:`, not on a fresh client (calling too
    /// early was the cause of the prior segfaults). The device to prefer is
    /// resolved INTERNALLY from `MTLREPLAYER_OVERRIDE_DEVICE_REGISTRY_ID` or a
    /// name-similarity path; it cannot be handed an `id<MTLDevice>`. Returns
    /// void. See `docs/findings/06-infrastructure.md`.
    pub fn GTMTLReplayClient_preferDevice(client: *mut GTMTLReplayClient);
    /// Harvester getters (dossier 02): the consumer side of a self-describing
    /// "capture" metadata block (magic `capture` at 0x00, u16 version at 0x08,
    /// u16 type at 0x0a, u32 metadataSize at 0x0c, u64 planeCount at 0x10, then
    /// 0x30-byte plane descriptors at 0x18 + i*0x30, then the data payload). No
    /// session or ObjC object: the "handle" is a raw (pointer, byte-length).
    ///
    /// Validates `block != null && size >= 0x10 && [block] == magic`; returns
    /// `block` when valid, else null. A `(buffer, size) -> header` cast.
    pub fn GTHarvesterGetMetadata(block: *const c_void, size: usize) -> *const c_void;
    /// Same validation, then returns `block + metadataSize` (the data after the
    /// metadata; metadataSize = `[block+0xc]`, +0x10 when version == 1).
    pub fn GTHarvesterGetData(block: *const c_void, size: usize) -> *const c_void;
    /// If the block's type (`[block+0xa]`) is 1 (texture), returns the plane
    /// count `[block+0x10]`, else 0. One argument, the metadata block.
    pub fn GTHarvesterGetTexturePlaneCount(block: *const c_void) -> u64;
    /// If type == 1, returns `block + 0x18 + index*0x30` (the i-th 0x30-byte
    /// plane descriptor), else 0. Two arguments: block, plane index.
    pub fn GTHarvesterGetTexturePlane(block: *const c_void, index: u64) -> *const c_void;
    /// The framework's global config/env table (dossier 06). A DATA symbol, not
    /// a function: `GTMTLReplayController_init` populates the config word at
    /// `GT_ENV + 0x30` from the `MTLREPLAYER_*` environment variables (each env
    /// var `bfi`-ing one bit). Declared opaque; take its address, never a value.
    pub static GT_ENV: c_void;
    /// Opens the NSXPC transport to the entitled `com.apple.gputools.replay`
    /// service (dossier 06; 1 arg, the client). Internally creates a pool,
    /// calls `GTMTLReplayClient_init` on the client, opens the connection, and
    /// wires up remote-object proxies. The transport-path client fields
    /// `preferDevice` needs (e.g. `[client+0x30]`) are populated only if that
    /// connection succeeds. Return not established; declared opaque.
    pub fn GTMTLReplayClient_createNewTransport(client: *mut GTMTLReplayClient) -> *mut c_void;
    /// objc_storeStrong(&_observer, x0): one argument, an object responding to
    /// `-notifyError:`. Must be called BEFORE `-load:error:` or replay
    /// failures are silent (HANDOFF 2.2).
    pub fn GTMTLReplayErrorHandling_initWithObserver(observer: *mut AnyObject);
    /// Controller playback: play to the end. Signature established by static
    /// disassembly of the extracted GPUToolsReplay framework; see
    /// docs/findings/01-playback.md. The body is 5 instructions and never
    /// reads x1/x2 as inputs: 1 arg (controller). It tail-calls
    /// `_GTMTLReplayController_debugSubCommandStop`, whose own return is not
    /// established, so this fn's return is not consumed; declared void
    /// (ABI-safe on AArch64 even if the callee leaves a value in w0).
    pub fn GTMTLReplayController_playAll(controller: *mut GTMTLReplayController);
    /// Controller playback: forward-only replay from the controller's current
    /// command index up to (not including) `target_index`. Signature
    /// established by static disassembly; see docs/findings/01-playback.md.
    /// Prologue saves x1->x19 and x0->x20: 2 args (controller, uint32 target
    /// command index). The exit path never deliberately sets a return value
    /// (x0 at `ret` is leftover register state from an unrelated call), so
    /// the return is not established; declared void.
    pub fn GTMTLReplayController_playTo(controller: *mut GTMTLReplayController, target_index: u32);
    /// Controller playback: rewind (teardown, then restore initial state).
    /// Signature established by static disassembly; see
    /// docs/findings/01-playback.md. Prologue saves x0->x19 and never reads
    /// x1: 1 arg (controller). It calls `_Rewind(controller)` via `bl` (not a
    /// tail call) and returns without touching x0, so it passes through
    /// `_Rewind`'s own return, which is itself not established; declared
    /// void.
    pub fn GTMTLReplayController_rewind(controller: *mut GTMTLReplayController);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Coercing each function to its typed pointer forces the linker to
    /// resolve the symbol. This test failing to LINK is the signal: an
    /// established symbol is no longer exported by the framework.
    #[test]
    fn established_symbols_resolve_at_link_time() {
        let _: unsafe extern "C" fn() -> c_int = apr_initialize;
        let _: unsafe extern "C" fn(
            *mut *mut AprPool,
            *mut AprPool,
            *mut c_void,
            *mut c_void,
        ) -> c_int = apr_pool_create_ex;
        let _: unsafe extern "C" fn(*mut AprPool) -> *mut c_void = GTMTLReplayController_init;
        let _: unsafe extern "C" fn(*mut GTMTLReplayClient, *mut AprPool) = GTMTLReplayClient_init;
        let _: unsafe extern "C" fn(*mut GTMTLReplayClient) = GTMTLReplayClient_preferDevice;
        let _: unsafe extern "C" fn(*const c_void, usize) -> *const c_void = GTHarvesterGetMetadata;
        let _: unsafe extern "C" fn(*const c_void, usize) -> *const c_void = GTHarvesterGetData;
        let _: unsafe extern "C" fn(*const c_void) -> u64 = GTHarvesterGetTexturePlaneCount;
        let _: unsafe extern "C" fn(*const c_void, u64) -> *const c_void =
            GTHarvesterGetTexturePlane;
        let _: unsafe extern "C" fn(*mut AnyObject) = GTMTLReplayErrorHandling_initWithObserver;
        let _: unsafe extern "C" fn(*mut GTMTLReplayController) = GTMTLReplayController_playAll;
        let _: unsafe extern "C" fn(*mut GTMTLReplayController, u32) = GTMTLReplayController_playTo;
        let _: unsafe extern "C" fn(*mut GTMTLReplayController) = GTMTLReplayController_rewind;
    }

    /// Drives all four harvester getters live against a synthetic "capture"
    /// block built to the layout established in `docs/findings/02-harvester.md`,
    /// and checks each returns exactly what the layout predicts. Ground-truth
    /// round-trip: the block is ours, so the expected outputs are known. This
    /// graduates the four exported harvester symbols from signature-established
    /// to behaviour-confirmed, and pins the block layout against the framework.
    #[test]
    fn the_harvester_getters_parse_a_synthetic_capture_block() {
        use core::ffi::c_void;
        const METADATA_SIZE: usize = 0x78; // header 0x18 + two 0x30 plane descs
        const PAYLOAD: &[u8] = b"HARVESTED_PAYLOAD";
        let total = METADATA_SIZE + PAYLOAD.len();
        // 8-aligned backing (the getters read [block] as a u64 magic).
        let mut words = vec![0u64; total.div_ceil(8)];
        let block = words.as_mut_ptr() as *mut u8;
        // SAFETY: `block` owns `words.len()*8 >= total` bytes; every write is
        // within `[0, total)` and correctly sized/aligned for its type.
        unsafe {
            block.copy_from(
                [0x65u8, 0x72, 0x75, 0x74, 0x70, 0x61, 0x63, 0x00].as_ptr(),
                8,
            );
            block.add(0x08).cast::<u16>().write(2); // version
            block.add(0x0a).cast::<u16>().write(1); // type = texture
            block.add(0x0c).cast::<u32>().write(METADATA_SIZE as u32);
            block.add(0x10).cast::<u64>().write(2); // planeCount
            for b in 0x18..0x48 {
                block.add(b).write(0xA0);
            }
            for b in 0x48..0x78 {
                block.add(b).write(0xB0);
            }
            block
                .add(METADATA_SIZE)
                .copy_from(PAYLOAD.as_ptr(), PAYLOAD.len());
        }
        let bv = block as *const c_void;
        // SAFETY: all four are the framework's block-parsing getters; `bv` is a
        // valid `capture` block of `total` bytes, per the writes above.
        unsafe {
            assert_eq!(GTHarvesterGetMetadata(bv, total), bv, "valid metadata");
            assert!(
                GTHarvesterGetMetadata(bv, 8).is_null(),
                "size < 0x10 rejected"
            );
            assert!(GTHarvesterGetMetadata(core::ptr::null(), total).is_null());
            assert_eq!(GTHarvesterGetTexturePlaneCount(bv), 2, "plane count");
            let p0 = GTHarvesterGetTexturePlane(bv, 0) as *const u8;
            let p1 = GTHarvesterGetTexturePlane(bv, 1) as *const u8;
            assert_eq!(p0, block.add(0x18).cast_const(), "plane 0 offset");
            assert_eq!(p1, block.add(0x48).cast_const(), "plane 1 offset");
            assert_eq!(p0.read(), 0xA0, "plane 0 contents");
            assert_eq!(p1.read(), 0xB0, "plane 1 contents");
            let data = GTHarvesterGetData(bv, total) as *const u8;
            assert_eq!(data, block.add(METADATA_SIZE).cast_const(), "data offset");
            let got = core::slice::from_raw_parts(data, PAYLOAD.len());
            assert_eq!(got, PAYLOAD, "data payload");
        }
        let mut bad = vec![0u64; total.div_ceil(8)];
        let badp = bad.as_mut_ptr() as *const c_void;
        // SAFETY: `bad` is zeroed (magic != "capture"); a read-only validate.
        unsafe {
            assert!(
                GTHarvesterGetMetadata(badp, total).is_null(),
                "bad magic rejected"
            );
        }
    }
}
