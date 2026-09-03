//! Live: needs the framework + captures/known-3d.gputrace
//! (fixture-apps/known-3d.m, force-loaded since the volume texture is never
//! sampled by a captured command). A 16x16x4 BGRA8Unorm 3D texture, blit-
//! stored.
//!
//! MEASURED here: `Texture::depth()` reads 1, NOT 4, for this volume
//! texture's fetch. This confirms (rather than contradicts)
//! docs/findings/00-texture-fetch.md's "3D volumes" finding:
//! `GTReplayFetchTexture` has no z-plane selector and always serves exactly
//! one fixed z-plane of a `Type3D` texture, so `depth()` is NOT a usable "is
//! this a 3D texture" signal from the fetch alone.
//!
//! Its own file (rather than `live_hl_provenance.rs`) because `Session` is
//! process-wide one-shot: this needs its own `Capture::open`.
//!
//! Build + capture (if not already present):
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/known-3d \
//!         fixture-apps/known-3d.m -framework Metal -framework Foundation
//!   fixture-apps/capture-late.sh /tmp/known-3d captures/known-3d.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay-hl --test live_hl_provenance_3d -- --ignored
//!
//! Its own test binary (separate process).

use gputools_replay_hl::{Capture, ReplayerConfig};

fn bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/known-3d.gputrace"
    ))
    .to_owned()
}

#[test]
#[ignore = "live: needs the framework and captures/known-3d.gputrace"]
fn depth_reports_one_fixed_z_plane_not_the_volume_depth() {
    // known-3d's volume texture is never sampled by a captured command, so
    // it must be force-loaded to answer.
    // SAFETY: single-threaded test process, before any other thread.
    unsafe {
        Capture::configure_env(&ReplayerConfig {
            force_load_unused_resources: true,
            ..ReplayerConfig::default()
        })
    };
    let cap = Capture::open(&bundle()).expect("open known-3d capture");

    let textures = cap.textures(0..=40).expect("sweep fetch textures");
    let vol = textures
        .iter()
        .find(|t| t.width() == 16 && t.height() == 16)
        .expect("no 16x16 texture in known-3d sweep 0..=40");
    assert_eq!(
        vol.depth(),
        1,
        "the fetch always serves one fixed z-plane of a 3D texture (docs/findings), \
         so depth() should read 1, not the source volume's real depth (4)"
    );
}
