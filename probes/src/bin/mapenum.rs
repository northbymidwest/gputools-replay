//! Enumerator spike (finding #4): what do the object map's enumeration methods
//! actually return? `-[GTMTLReplayObjectMap resources]` is a getter for the ivar
//! at +0x108 - this checks whether that (and `-unusedResourceKeys`) is a KEYED
//! collection (streamRefs, which is what a consumer needs) or just objects.
//! Inspected right after open (map populated; no playback).
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh mapenum [path-to.gputrace]

use gputools_replay_sys::layout::controller_object_map;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, sel};
use probes::{guard, session};
use std::ffi::CStr;
use std::path::PathBuf;
use std::process::ExitCode;

/// The runtime class name of an object (no message send).
fn class_name(obj: &AnyObject) -> String {
    // SAFETY: `obj` is a live object; object_getClassName reads its isa.
    let name = unsafe { objc2::ffi::object_getClassName(obj) };
    if name.is_null() {
        return "(null class)".into();
    }
    unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned()
}

fn responds(obj: &AnyObject, s: objc2::runtime::Sel) -> bool {
    unsafe { msg_send![obj, respondsToSelector: s] }
}

/// Dump one enumeration result: class, count, and (if keyed) its keys as u64.
fn inspect(label: &str, obj: Option<&AnyObject>) {
    let Some(obj) = obj else {
        println!("{label}: nil");
        return;
    };
    println!("{label}: class={}", class_name(obj));
    if responds(obj, sel!(count)) {
        let count: usize = unsafe { msg_send![obj, count] };
        println!("  count={count}");
    }
    // Keyed? NSDictionary answers allKeys + allValues.
    if responds(obj, sel!(allKeys)) {
        let keys: Option<Retained<AnyObject>> = unsafe { msg_send![obj, allKeys] };
        let vals: Option<Retained<AnyObject>> = unsafe { msg_send![obj, allValues] };
        if let (Some(keys), Some(vals)) = (keys, vals) {
            let n: usize = unsafe { msg_send![&*keys, count] };
            println!("  KEYED (streamRef -> resource):");
            for i in 0..n.min(24) {
                let k: Option<Retained<AnyObject>> = unsafe { msg_send![&*keys, objectAtIndex: i] };
                let v: Option<Retained<AnyObject>> = unsafe { msg_send![&*vals, objectAtIndex: i] };
                let ref_ = k
                    .as_deref()
                    .filter(|k| responds(k, sel!(unsignedLongLongValue)))
                    .map(|k| unsafe { msg_send![k, unsignedLongLongValue] })
                    .unwrap_or(u64::MAX);
                let cls = v.as_deref().map(class_name).unwrap_or_else(|| "?".into());
                println!("    {ref_:>4} -> {cls}");
            }
        }
    } else if responds(obj, sel!(allObjects)) {
        // NSSet: dump members (streamRefs).
        let objs: Option<Retained<AnyObject>> = unsafe { msg_send![obj, allObjects] };
        if let Some(objs) = objs {
            let n: usize = unsafe { msg_send![&*objs, count] };
            print!("  SET members ({n}): ");
            for i in 0..n.min(32) {
                let e: Option<Retained<AnyObject>> = unsafe { msg_send![&*objs, objectAtIndex: i] };
                let v = e
                    .as_deref()
                    .filter(|e| responds(e, sel!(unsignedLongLongValue)))
                    .map(|e| unsafe { msg_send![e, unsignedLongLongValue] })
                    .unwrap_or(u64::MAX);
                print!("{v} ");
            }
            println!();
        }
    } else if responds(obj, sel!(objectAtIndex:)) {
        // Flat array of objects (no keys). Sample element classes.
        let n: usize = unsafe { msg_send![obj, count] };
        print!("  FLAT array - first element classes: ");
        for i in 0..n.min(8) {
            let e: Option<Retained<AnyObject>> = unsafe { msg_send![obj, objectAtIndex: i] };
            if let Some(e) = e {
                print!("{} ", class_name(&e));
            }
        }
        println!();
    } else {
        println!("  (neither allKeys nor objectAtIndex: - some other container)");
    }
}

fn main() -> ExitCode {
    // SAFETY: single-threaded at the first line of main.
    unsafe { guard::set_unlock_env() };
    let bundle: PathBuf = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "captures/known-textures-late.gputrace".to_owned())
        .into();
    let sess = match session::Session::open(&bundle) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("mapenum FAILED to open: {e}");
            return ExitCode::FAILURE;
        }
    };
    // SAFETY: `open` ran init + load:, so the controller is live and loaded.
    let map = match unsafe { controller_object_map(sess.controller_in_client()) } {
        Ok(m) => m,
        Err(e) => {
            eprintln!("object map: {e}");
            return ExitCode::FAILURE;
        }
    };

    let resources: Option<Retained<AnyObject>> = unsafe { msg_send![&*map, resources] };
    inspect("resources", resources.as_deref());

    let unused: Option<Retained<AnyObject>> = unsafe { msg_send![&*map, unusedResourceKeys] };
    inspect("unusedResourceKeys", unused.as_deref());

    ExitCode::SUCCESS
}
