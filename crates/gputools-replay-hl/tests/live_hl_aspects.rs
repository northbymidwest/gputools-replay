//! Live: needs the framework + captures/known-depth-stencil.gputrace
//! (fixture-apps/known-depth-stencil.m). Renders a full-screen triangle at
//! known depth (0.5) and stencil ref (42) into a combined
//! `Depth32Float_Stencil8` attachment, blit-stored, so the depth and stencil
//! aspects are both selectable from the same streamRef via `Aspect`.
//!
//! Build + capture (if not already present):
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/known-depth-stencil \
//!         fixture-apps/known-depth-stencil.m -framework Metal -framework Foundation
//!   fixture-apps/capture-late.sh /tmp/known-depth-stencil captures/known-depth-stencil.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay-hl --test live_hl_aspects -- --ignored
//!
//! Its own test binary (separate process).

use gputools_replay_hl::format::{DepthKind, FormatKind, StencilKind};
use gputools_replay_hl::{Aspect, Capture, MTLPixelFormat, ReplayerConfig};

fn bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/known-depth-stencil.gputrace"
    ))
    .to_owned()
}

#[test]
#[ignore = "live: needs the framework and captures/known-depth-stencil.gputrace"]
fn combined_depth_stencil_aspects_via_domain_api() {
    // SAFETY: single-threaded test process, before any other thread.
    unsafe { Capture::configure_env(&ReplayerConfig::default()) };
    let cap = Capture::open(&bundle()).expect("open known-depth-stencil capture");

    // Depth aspect (plane 0): as with the other combined-format fixtures, a
    // same-format non-snapshotted sibling reads back uninitialized (NaN)
    // bytes, so filter to Depth32Float first, then select by content.
    let depths = cap
        .texture_aspects(0..=40, Aspect::Depth)
        .expect("fetch depth aspect textures");
    let (depth, _depth_px) = depths
        .iter()
        .filter(|t| t.format() == MTLPixelFormat::Depth32Float)
        .find_map(|t| {
            let px = t.pixels::<f32>().ok()?;
            (!px.is_empty() && px.iter().all(|&v| v == 0.5)).then_some((t, px))
        })
        .expect("no Depth32Float depth-aspect texture reading 0.5 everywhere");

    match depth.format_kind() {
        FormatKind::DepthStencil(d) => {
            assert_eq!(d.depth, Some(DepthKind::Float32));
            assert_eq!(d.stencil, None);
        }
        other => panic!("expected DepthStencil format_kind, got {other:?}"),
    }

    // Stencil aspect (plane 1), same streamRef family: filter to
    // X32_Stencil8 (fmt 261) first, then select by content (== 42).
    let stencils = cap
        .texture_aspects(0..=40, Aspect::Stencil)
        .expect("fetch stencil aspect textures");
    let (stencil, _stencil_px) = stencils
        .iter()
        .filter(|t| t.format() == MTLPixelFormat::X32_Stencil8)
        .find_map(|t| {
            let px = t.pixels::<u8>().ok()?;
            (!px.is_empty() && px.iter().all(|&v| v == 42)).then_some((t, px))
        })
        .expect("no X32_Stencil8 stencil-aspect texture reading 42 everywhere");

    match stencil.format_kind() {
        FormatKind::DepthStencil(d) => {
            assert_eq!(d.stencil, Some(StencilKind::Uint8));
            assert_eq!(d.depth, None);
        }
        other => panic!("expected DepthStencil format_kind, got {other:?}"),
    }
}
