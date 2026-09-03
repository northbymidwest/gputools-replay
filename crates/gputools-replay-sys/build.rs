//! Links the private framework from the RUNTIME path (HANDOFF section 1): no
//! dlopen, no libc. The linker resolves GPUToolsReplay from the running
//! system's dyld shared cache, so this path inherently matches the OS the
//! crate will run on. The crate targets macOS 27 (the ABI was read from
//! disassembly there; the 26 SDK stub is even missing
//! GTReplayFetchAccelerationStructure), so the build refuses older hosts.

use std::process::Command;

/// The framework is an OS-installed private framework resolved from the running
/// system's dyld shared cache; cross-compiling it is not a supported use case.
const MIN_MAJOR: u32 = 27;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Register the cfg we may set below so the crate's `cfg(docsrs)` uses do
    // not trip the unexpected-cfg lint on a normal build.
    println!("cargo:rustc-check-cfg=cfg(docsrs)");

    // docs.rs builds documentation on Linux and cannot host the private
    // framework. Detect it (docs.rs sets `DOCS_RS`) and emit no link
    // directives and no host-version gate, so `cargo doc` renders the API
    // there without the framework present. The `docsrs` cfg it also enables
    // is what the crate keys its `#[cfg_attr(docsrs, doc(cfg(...)))]` on.
    if std::env::var_os("DOCS_RS").is_some() {
        println!("cargo:rustc-cfg=docsrs");
        return;
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "macos" {
        panic!(
            "gputools-replay-sys only builds for macOS (target_os = {target_os:?}); \
             it links a macOS-only private framework."
        );
    }

    let major = host_macos_major();
    assert!(
        major >= MIN_MAJOR,
        "gputools-replay-sys requires macOS {MIN_MAJOR} or newer (found major version \
         {major}). The framework ABI was established on macOS 27; older systems ship a \
         different GPUToolsReplay."
    );

    println!("cargo:rustc-link-search=framework=/System/Library/PrivateFrameworks");
    println!("cargo:rustc-link-lib=framework=GPUToolsReplay");
    println!("cargo:rustc-link-lib=dylib=apr-1.0");
}

/// Host macOS major version via `sw_vers -productVersion`. Read from the host,
/// not from an SDK path: `xcrun` SDK-version resolution is unreliable here (on
/// this machine `--show-sdk-path` gives the 27.0 SDK while `--sdk macosx
/// --show-sdk-version` reports 26.5).
fn host_macos_major() -> u32 {
    let out = Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .expect("failed to run sw_vers to determine macOS version");
    let version = String::from_utf8(out.stdout).expect("sw_vers output was not UTF-8");
    version
        .trim()
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("could not parse macOS version from {version:?}"))
}
