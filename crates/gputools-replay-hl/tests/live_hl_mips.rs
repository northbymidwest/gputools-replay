//! Live: needs the framework + captures/known-mips.gputrace
//! (fixture-apps/known-mips.m, force-loaded since the array texture is never
//! sampled). A 2D-array (slice0 red, slice1 green) mipmapped BGRA8Unorm
//! texture, mips generated + blit-stored.
//!
//! Build + capture (if not already present):
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/known-mips \
//!         fixture-apps/known-mips.m -framework Metal -framework Foundation
//!   fixture-apps/capture-late.sh /tmp/known-mips captures/known-mips.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay-hl --test live_hl_mips -- --ignored
//!
//! Its own test binary (separate process).

use gputools_replay::request::TextureRequest;
use gputools_replay_hl::{Capture, ReplayerConfig};

fn bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/known-mips.gputrace"
    ))
    .to_owned()
}

#[test]
#[ignore = "live: needs the framework and captures/known-mips.gputrace"]
fn slice_selects_array_slice_via_domain_api() {
    // known-mips' array texture is never sampled by a captured command, so it
    // must be force-loaded to answer.
    // SAFETY: single-threaded test process, before any other thread.
    unsafe {
        Capture::configure_env(&ReplayerConfig {
            force_load_unused_resources: true,
            ..ReplayerConfig::default()
        })
    };
    let cap = Capture::open(&bundle()).expect("open known-mips capture");

    // Find the fixture's 64x64 array texture streamRef: sweep slice-0/level-0
    // (the default natural fetch) and take the first record at the base
    // size. streamRefs are sparse, so sweep rather than guess an index.
    let sweep = cap.textures(0..=40).expect("sweep fetch textures");
    let base = sweep
        .iter()
        .find(|t| t.width() == 64 && t.height() == 64)
        .expect("no 64x64 texture in known-mips sweep 0..=40");
    let stream_ref = base.stream_ref();

    let slice0 = TextureRequest::natural(stream_ref).with_slice_level(0, 0);
    let slice1 = TextureRequest::natural(stream_ref).with_slice_level(1, 0);

    let r0 = cap.textures_with(&[slice0]).expect("fetch slice 0");
    let px0 = r0[0].pixels::<[u8; 4]>().expect("slice 0 pixels");
    assert_eq!(
        px0[0],
        [0, 0, 255, 255],
        "slice 0 should decode to red (BGRA)"
    );

    let r1 = cap.textures_with(&[slice1]).expect("fetch slice 1");
    let px1 = r1[0].pixels::<[u8; 4]>().expect("slice 1 pixels");
    assert_eq!(
        px1[0],
        [0, 255, 0, 255],
        "slice 1 should decode to green (BGRA)"
    );
}
