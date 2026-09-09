//! Validate the session-descriptor path (phase 2): reach `GTMTLReplayObjectMap`
//! via the sys accessor (`controller + 0x8`), then read each loaded texture's
//! descriptor through `tryGetTextureForKey:` + the public Metal API.
//!
//! The offset was originally discovered by a `malloc_size`-guarded heap scan
//! (recorded in docs/design/2026-09-04-...); the `-sys` accessor now does that
//! validation itself and returns a typed, retained map, so this dogfoods it.
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh objectmap [path-to.gputrace]

use gputools_replay_sys::layout::controller_object_map;
use objc2_metal::MTLTexture;
use probes::{guard, session};
use std::path::PathBuf;
use std::process::ExitCode;

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
    // The accessor validates controller+0x8 (live heap object of the right
    // class) and returns the retained, typed map, or an error naming the check
    // that failed if the measured offset has moved.
    let map = match unsafe { controller_object_map(controller) } {
        Ok(map) => map,
        Err(e) => {
            eprintln!("object map at controller+0x8: {e}");
            return ExitCode::FAILURE;
        }
    };

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
