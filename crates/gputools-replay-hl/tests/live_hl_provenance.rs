//! Live: needs the framework + captures/known-mips.gputrace
//! (fixture-apps/known-mips.m, force-loaded since the array texture is never
//! sampled). Proves `Texture::plane()/slice()/level()/depth()` are carried
//! through from the reply record's self-describing fields (info 0x48/0x4c -
//! docs/findings/00-texture-fetch.md), via `Capture::textures_with`.
//!
//! Build + capture (if not already present):
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/known-mips \
//!         fixture-apps/known-mips.m -framework Metal -framework Foundation
//!   fixture-apps/capture-late.sh /tmp/known-mips captures/known-mips.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay-hl --test live_hl_provenance -- --ignored
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
fn texture_provenance_via_domain_api() {
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

    // A natural fetch reports 0/0/0 (plane0/slice0/level0), and depth() == 1
    // for this 2D-array texture (each slice/level is its own 2D image).
    let sweep = cap.textures(0..=40).expect("sweep fetch textures");
    let base = sweep
        .iter()
        .find(|t| t.width() == 64 && t.height() == 64)
        .expect("no 64x64 texture in known-mips sweep 0..=40");
    assert_eq!(base.plane(), 0, "natural fetch should report plane 0");
    assert_eq!(base.slice(), 0, "natural fetch should report slice 0");
    assert_eq!(base.level(), 0, "natural fetch should report level 0");
    assert_eq!(base.depth(), 1, "a 2D texture's fetched depth should be 1");
    let stream_ref = base.stream_ref();

    // Specific (slice, level) requests: the returned Texture's slice()/level()
    // must match what was asked for. Locate each expected result by its
    // self-described (slice, level) rather than by position in `results` -
    // reply order matching request order is not an established invariant,
    // and self-description is the actual feature under test here.
    let slice1_level0 = TextureRequest::natural(stream_ref).with_slice_level(1, 0);
    let slice0_level1 = TextureRequest::natural(stream_ref).with_slice_level(0, 1);
    let results = cap
        .textures_with(&[slice1_level0, slice0_level1])
        .expect("fetch specific slice/level requests");

    let t_slice1 = results
        .iter()
        .find(|t| t.slice() == 1 && t.level() == 0)
        .expect("no texture self-described as slice 1 / level 0");
    assert_eq!(t_slice1.slice(), 1, "should self-describe slice 1");
    assert_eq!(t_slice1.level(), 0, "should self-describe level 0");

    let t_level1 = results
        .iter()
        .find(|t| t.slice() == 0 && t.level() == 1)
        .expect("no texture self-described as slice 0 / level 1");
    assert_eq!(t_level1.slice(), 0, "should self-describe slice 0");
    assert_eq!(t_level1.level(), 1, "should self-describe level 1");
    assert_eq!(
        t_level1.width(),
        32,
        "level 1 should be the halved mip (64 -> 32)"
    );
}
