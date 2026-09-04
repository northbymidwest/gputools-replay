//! Find `GTMTLReplayObjectMap` in the loaded replay state and confirm
//! `tryGetTextureForKey:` returns live, typed `MTLTexture`s. RE probe for the
//! session-based-descriptor design (docs/design/2026-09-04-...).
//!
//! The map is not in the ObjC service graph; it hangs off the controller (the C
//! struct at client field 1). So this scans the controller's heap-reachable
//! graph for it. Safety: every candidate pointer is validated with
//! `malloc_size` (returns 0 for a non-heap pointer, never faults) before it is
//! read, and the map is identified by comparing the class POINTER (no class
//! dereference). Dereferencing an unvalidated pointer faults into the
//! framework's HandleCrashSignal and hangs, so we never do it.
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh objectmap [path-to.gputrace]

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use probes::{guard, session};
use std::collections::HashSet;
use std::ffi::{CStr, c_char, c_void};
use std::path::PathBuf;
use std::process::ExitCode;

unsafe extern "C" {
    fn malloc_size(ptr: *const c_void) -> usize;
    fn object_getClass(obj: *const c_void) -> *const AnyClass;
    fn class_getName(cls: *const AnyClass) -> *const c_char;
}

fn class_name_of(cls: *const AnyClass) -> String {
    if cls.is_null() {
        return "(null class)".to_owned();
    }
    // SAFETY: cls is a class pointer returned by object_getClass on a validated
    // heap block; class_getName is total on it.
    unsafe {
        CStr::from_ptr(class_getName(cls))
            .to_string_lossy()
            .into_owned()
    }
}

/// Scan the heap graph reachable from `root` (a heap block address) for an
/// object of class `target`. Returns the found object address and prints where.
/// All reads are guarded by `malloc_size`, so no unvalidated pointer is touched.
fn scan_for(root: usize, root_size: usize, target: *const AnyClass) -> Option<usize> {
    let mut seen: HashSet<usize> = HashSet::new();
    // (block, size, depth); the root carries a known-safe size, children use
    // malloc_size (they are heap-allocated ObjC objects).
    let mut queue: Vec<(usize, usize, usize)> = vec![(root, root_size, 0)];
    let mut blocks = 0usize;
    while let Some((block, size, depth)) = queue.pop() {
        if !seen.insert(block) || size == 0 || size > 262_144 {
            continue;
        }
        blocks += 1;
        if blocks > 20_000 {
            println!("(scan cap reached)");
            break;
        }
        for i in 0..size / 8 {
            // SAFETY: reading within a live block of `size` bytes.
            let w = unsafe { ((block + i * 8) as *const usize).read() };
            if w == 0 {
                continue;
            }
            // SAFETY: malloc_size returns 0 for a non-heap pointer without
            // faulting; we never deref `w` unless it is a live heap block.
            let wsize = unsafe { malloc_size(w as *const c_void) };
            if wsize == 0 {
                continue;
            }
            let cls = unsafe { object_getClass(w as *const c_void) };
            if cls == target {
                if depth == 0 {
                    println!(
                        "FOUND: controller+0x{:x} -> GTMTLReplayObjectMap 0x{w:x}",
                        i * 8
                    );
                } else {
                    println!("FOUND GTMTLReplayObjectMap 0x{w:x} at depth {depth}");
                }
                return Some(w);
            }
            if depth < 6 {
                queue.push((w, wsize, depth + 1));
            }
        }
    }
    println!(
        "GTMTLReplayObjectMap not found in the controller heap graph ({blocks} blocks scanned)"
    );
    None
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

    let target = match AnyClass::get(c"GTMTLReplayObjectMap") {
        Some(c) => c as *const AnyClass,
        None => {
            eprintln!("GTMTLReplayObjectMap class not registered");
            return ExitCode::FAILURE;
        }
    };
    let controller = sess.controller_in_client() as usize;
    // The controller is provably >= 0x5824 bytes (command_index at 0x5820).
    // Use its malloc size if it is a malloc block, else that known floor.
    let root_size = {
        let s = unsafe { malloc_size(controller as *const c_void) };
        if s > 0 { s.min(0x8000) } else { 0x5820 }
    };
    println!(
        "controller 0x{controller:x} (scan {root_size:#x}), map class 0x{:x}",
        target as usize
    );

    let map = match scan_for(controller, root_size, target) {
        Some(m) => m as *mut AnyObject,
        None => return ExitCode::FAILURE,
    };

    let mut hits = 0usize;
    // SAFETY: map is the confirmed, non-null GTMTLReplayObjectMap.
    let map_ref = unsafe { &*map };
    for key in 0u64..512 {
        // SAFETY: tryGetTextureForKey: is a pure lookup returning id<MTLTexture>
        // or nil; objc2 retains the result. The getters are +0 NSUInteger props.
        let tex: Option<Retained<AnyObject>> =
            unsafe { msg_send![map_ref, tryGetTextureForKey: key] };
        let Some(tex) = tex else { continue };
        hits += 1;
        unsafe {
            let width: usize = msg_send![&*tex, width];
            let height: usize = msg_send![&*tex, height];
            let pixel_format: usize = msg_send![&*tex, pixelFormat];
            let texture_type: usize = msg_send![&*tex, textureType];
            let mips: usize = msg_send![&*tex, mipmapLevelCount];
            let array_len: usize = msg_send![&*tex, arrayLength];
            let cls = object_getClass(Retained::as_ptr(&tex) as *const c_void);
            println!(
                "  streamRef {key}: {} {width}x{height} fmt={pixel_format} type={texture_type} mips={mips} array={array_len}",
                class_name_of(cls)
            );
        }
    }
    println!("textures via tryGetTextureForKey: {hits}");
    ExitCode::SUCCESS
}
