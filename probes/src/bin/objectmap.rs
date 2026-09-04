//! Validate the session-descriptor path (phase 2): reach `GTMTLReplayObjectMap`
//! via the sys accessor (`controller + 0x8`), verify its class defensively,
//! then read each loaded texture's descriptor through `tryGetTextureForKey:` +
//! the public Metal API.
//!
//! The offset was originally discovered by a `malloc_size`-guarded heap scan
//! (recorded in docs/design/2026-09-04-...); this now dogfoods the typed
//! `-sys` binding instead.
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh objectmap [path-to.gputrace]

use gputools_replay_sys::replay::{GTMTLReplayObjectMap, controller_object_map};
use objc2::ClassType;
use objc2::runtime::AnyClass;
use objc2_metal::MTLTexture;
use probes::{guard, session};
use std::ffi::c_void;
use std::path::PathBuf;
use std::process::ExitCode;

unsafe extern "C" {
    fn malloc_size(ptr: *const c_void) -> usize;
    fn object_getClass(obj: *const c_void) -> *const AnyClass;
}

fn main() -> ExitCode {
    // SAFETY: single-threaded at the first line of main.
    unsafe { guard::set_unlock_env() };
    let bundle: PathBuf = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "captures/small.gputrace".to_owned())
        .into();
    let sess = match session::Session::open(&bundle) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("objectmap FAILED to open: {e}");
            return ExitCode::FAILURE;
        }
    };

    let controller = sess.controller_in_client();
    // SAFETY: `open` ran init + load:, so the controller is live and loaded.
    let map_ptr = unsafe { controller_object_map(controller) };
    println!("controller_object_map(controller+0x8) = {map_ptr:?}");
    // Defensive: the offset is MEASURED, so guard it before trusting the object
    // - a non-heap or wrong-class pointer means the layout moved.
    // SAFETY: malloc_size returns 0 for a non-heap pointer without faulting.
    if map_ptr.is_null() || unsafe { malloc_size(map_ptr as *const c_void) } == 0 {
        eprintln!("object map at controller+0x8 is not a live heap object");
        return ExitCode::FAILURE;
    }
    // SAFETY: map_ptr is a live heap object; reading its isa does not mutate it.
    let cls = unsafe { object_getClass(map_ptr as *const c_void) };
    if !std::ptr::eq(cls, GTMTLReplayObjectMap::class()) {
        eprintln!("controller+0x8 is not a GTMTLReplayObjectMap");
        return ExitCode::FAILURE;
    }
    // SAFETY: confirmed above to be a GTMTLReplayObjectMap.
    let map: &GTMTLReplayObjectMap = unsafe { &*(map_ptr as *const GTMTLReplayObjectMap) };

    let mut hits = 0usize;
    for key in 0u64..512 {
        let Some(tex) = map.try_get_texture(key) else {
            continue;
        };
        hits += 1;
        println!(
            "  streamRef {key}: {}x{} fmt={:?} type={:?} mips={} array={}",
            tex.width(),
            tex.height(),
            tex.pixelFormat(),
            tex.textureType(),
            tex.mipmapLevelCount(),
            tex.arrayLength(),
        );
    }
    println!("textures via try_get_texture: {hits}");
    ExitCode::SUCCESS
}
