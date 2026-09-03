//! Live: needs the framework + captures/known-stencil.gputrace
//! (fixture-apps/known-stencil.m). Renders stencil 42 into a base Stencil8
//! texture and stencil 77 + depth 0.5 into a combined Depth32Float_Stencil8
//! (with an X32_Stencil8 view), blit-stored.
//!
//! Build + capture (if not already present):
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/known-stencil \
//!         fixture-apps/known-stencil.m -framework Metal -framework Foundation
//!   fixture-apps/capture-late.sh /tmp/known-stencil captures/known-stencil.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay-hl --test live_hl_stencil -- --ignored
//!
//! Its own test binary (separate process).

use gputools_replay_hl::format::{DepthKind, FormatKind, StencilKind};
use gputools_replay_hl::{Capture, MTLPixelFormat, ReplayerConfig};

fn bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/known-stencil.gputrace"
    ))
    .to_owned()
}

#[test]
#[ignore = "live: needs the framework and captures/known-stencil.gputrace"]
fn stencil_textures_via_domain_api() {
    // SAFETY: single-threaded test process, before any other thread.
    unsafe { Capture::configure_env(&ReplayerConfig::default()) };
    let cap = Capture::open(&bundle()).expect("open known-stencil capture");

    let textures = cap.textures(0..=64).expect("fetch textures");

    // The fixture creates a Stencil8 render target and a second Stencil8
    // texture to blit it into; both fetch with the same format, but only
    // the one whose content was actually snapshotted at the capture
    // boundary reads real data (42) - the other reads back uninitialized
    // private-storage bytes. Same story for the combined format's depth
    // aspect (see live_hl_depth). Select by content, not position.
    let (base, _base_px) = textures
        .iter()
        .filter(|t| t.format() == MTLPixelFormat::Stencil8)
        .find_map(|t| {
            let px = t.pixels::<u8>().ok()?;
            (!px.is_empty() && px.iter().all(|&v| v == 42)).then_some((t, px))
        })
        .expect("no Stencil8 texture reading 42 everywhere");

    // Independent of the pixel-value selection above: the base texture's
    // format classifies as a stencil-only DepthStencil format.
    match base.format_kind() {
        FormatKind::DepthStencil(d) => {
            assert_eq!(d.stencil, Some(StencilKind::Uint8));
            assert_eq!(d.depth, None);
        }
        other => panic!("expected DepthStencil format_kind, got {other:?}"),
    }

    // The combined format's depth aspect surfaces as an ordinary
    // Depth32Float texture, reading 0.5.
    let (depth, _depth_px) = textures
        .iter()
        .filter(|t| t.format() == MTLPixelFormat::Depth32Float)
        .find_map(|t| {
            let px = t.pixels::<f32>().ok()?;
            (!px.is_empty() && px.iter().all(|&v| v == 0.5)).then_some((t, px))
        })
        .expect("no Depth32Float (combined depth aspect) texture reading 0.5 everywhere");

    // Independent of the pixel-value selection above: the combined format's
    // depth aspect classifies as a depth-only DepthStencil format.
    match depth.format_kind() {
        FormatKind::DepthStencil(d) => {
            assert_eq!(d.depth, Some(DepthKind::Float32));
            assert_eq!(d.stencil, None);
        }
        other => panic!("expected DepthStencil format_kind, got {other:?}"),
    }
}
