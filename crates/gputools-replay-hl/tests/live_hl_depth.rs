//! Live: needs the framework + captures/known-depth.gputrace
//! (fixture-apps/known-depth.m). Renders a full-screen triangle at a known
//! constant depth (0.5) into a Depth32Float attachment, then blits it to a
//! second depth texture so the rendered content is snapshotted.
//!
//! Build + capture (if not already present):
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/known-depth \
//!         fixture-apps/known-depth.m -framework Metal -framework Foundation
//!   fixture-apps/capture-late.sh /tmp/known-depth captures/known-depth.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay-hl --test live_hl_depth -- --ignored
//!
//! Its own test binary (separate process).

use gputools_replay_hl::format::{DepthKind, FormatKind};
use gputools_replay_hl::{Capture, MTLPixelFormat, ReplayerConfig};

fn bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/known-depth.gputrace"
    ))
    .to_owned()
}

#[test]
#[ignore = "live: needs the framework and captures/known-depth.gputrace"]
fn depth_texture_via_domain_api() {
    // SAFETY: single-threaded test process, before any other thread.
    unsafe { Capture::configure_env(&ReplayerConfig::default()) };
    let cap = Capture::open(&bundle()).expect("open known-depth capture");

    let textures = cap.textures(0..=64).expect("fetch textures");
    // Both endpoints of the fixture's blit fetch as Depth32Float, but only
    // the endpoint whose content was actually snapshotted at the capture
    // boundary carries real data (0.5 everywhere); the other reads back
    // uninitialized private-storage bytes (NaN) - a known replayer
    // limitation (see known-depth.m). Select by content, not position.
    let (depth, _px) = textures
        .iter()
        .filter(|t| t.format() == MTLPixelFormat::Depth32Float)
        .find_map(|t| {
            let px = t.pixels::<f32>().ok()?;
            (!px.is_empty() && px.iter().all(|&v| v == 0.5)).then_some((t, px))
        })
        .expect("no Depth32Float texture reading 0.5 everywhere");

    // Independent of the pixel-value selection above: the format classifies
    // as a depth-only DepthStencil format.
    match depth.format_kind() {
        FormatKind::DepthStencil(d) => {
            assert_eq!(d.depth, Some(DepthKind::Float32));
            assert_eq!(d.stencil, None);
        }
        other => panic!("expected DepthStencil format_kind, got {other:?}"),
    }
}
