//! Links the private framework from the RUNTIME path (HANDOFF section 1): no
//! dlopen, no libc. The linker resolves GPUToolsReplay from the running
//! system's dyld shared cache, so this path inherently matches the OS the
//! crate will run on. The ABI was reverse-engineered on macOS 27, so the
//! default `macos27` feature floors the build there. Disabling it
//! (`--no-default-features`) lowers the floor to macOS 26, which ships the same
//! framework minus the `GTReplayFetchAccelerationStructure` class (so
//! acceleration-structure fetch returns a `Setup` error there - it is a runtime
//! class lookup, not a link dependency, so nothing else is affected). Systems
//! older than the floor are untested (we have no data; they may or may not be
//! compatible) and are refused conservatively, not because they are known to
//! differ.

use std::process::Command;

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

    // The default `macos27` feature floors the build at macOS 27 (where the ABI
    // was established); disabling it lowers the floor to 26. Build scripts see
    // enabled features as CARGO_FEATURE_<NAME> env vars.
    let min_major = if std::env::var_os("CARGO_FEATURE_MACOS27").is_some() {
        27
    } else {
        26
    };
    let major = host_macos_major();
    assert!(
        major >= min_major,
        "gputools-replay-sys requires macOS {min_major} or newer (found major version \
         {major}). The ABI was established on macOS 27; build with --no-default-features \
         to also build on macOS 26. Older systems are untested (we have no data), so they \
         are refused conservatively rather than assumed compatible."
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
