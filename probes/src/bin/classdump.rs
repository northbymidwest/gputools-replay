//! Dumps every registered GT* ObjC class: methods with type encodings and
//! instance sizes. Sessionless and safe to run any time.
//!
//! Usage: cargo run -p replay-probes --bin classdump [name-substring]

use objc2::runtime::{AnyClass, Method};
use std::ffi::CStr;

fn main() {
    // Force the sys crate (and thus the framework) to link so its classes
    // register before we enumerate.
    let (established, total) = gputools_replay_sys::inventory::coverage();
    eprintln!("gputools-replay-sys coverage: {established}/{total} symbols established");

    let filter = std::env::args().nth(1);

    // `AnyClass::classes()` wraps `objc_copyClassList` and frees the
    // returned array itself when the slice drops, so no manual free here.
    let classes = AnyClass::classes();
    let mut names: Vec<String> = Vec::new();
    for cls in classes.iter() {
        let name = cls.name().to_string_lossy().into_owned();
        if !name.starts_with("GT") {
            continue;
        }
        if let Some(f) = &filter
            && !name.contains(f.as_str())
        {
            continue;
        }
        names.push(name);
    }

    names.sort();
    for name in &names {
        dump_class(name);
    }
    eprintln!("\n{} GT* classes dumped", names.len());
}

fn dump_class(name: &str) {
    let cname = std::ffi::CString::new(name).unwrap();
    let Some(cls) = AnyClass::get(&cname) else {
        return;
    };
    println!("\n=== {name} (instanceSize {}) ===", cls.instance_size());
    for method in cls.instance_methods().iter() {
        let sel = method.name();
        let enc = method_type_encoding(method);
        println!("  -{}  {enc}", sel.name().to_string_lossy());
    }
}

/// `Method::types()` is a private iterator in objc2 0.6, so we go straight
/// to the runtime function it wraps.
fn method_type_encoding(method: &Method) -> String {
    // SAFETY: `method` is a valid `&Method` borrowed from a class's live
    // method list, so it points at a real objc_method for the duration of
    // this call. `method_getTypeEncoding` returns a NUL-terminated static
    // string owned by the runtime (not something we must free).
    let ptr = unsafe { objc2::ffi::method_getTypeEncoding(method) };
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: `ptr` was just checked non-null and comes from the runtime,
    // which guarantees a valid NUL-terminated C string here.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}
