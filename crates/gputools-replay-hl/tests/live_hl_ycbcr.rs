//! Live: needs the framework + captures/known-ycbcr.gputrace
//! (fixture-apps/known-ycbcr.m). A 64x64 biplanar 4:2:0 CVPixelBuffer
//! (Y=128, Cb=100, Cr=150) wrapped as per-plane MTLTextures.
//!
//! Build + capture (if not already present):
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/known-ycbcr \
//!         fixture-apps/known-ycbcr.m -framework Metal -framework Foundation \
//!         -framework CoreVideo
//!   fixture-apps/capture-late.sh /tmp/known-ycbcr captures/known-ycbcr.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay-hl --test live_hl_ycbcr -- --ignored
//!
//! Its own test binary (separate process).

use gputools_replay_hl::{Capture, MTLPixelFormat, ReplayerConfig};

fn bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/known-ycbcr.gputrace"
    ))
    .to_owned()
}

#[test]
#[ignore = "live: needs the framework and captures/known-ycbcr.gputrace"]
fn ycbcr_planes_via_domain_api() {
    // SAFETY: single-threaded test process, before any other thread.
    unsafe { Capture::configure_env(&ReplayerConfig::default()) };
    let cap = Capture::open(&bundle()).expect("open known-ycbcr capture");

    let textures = cap.textures(0..=64).expect("fetch textures");

    // Luma plane: R8Unorm, all 128.
    let luma = textures
        .iter()
        .find(|t| t.format() == MTLPixelFormat::R8Unorm)
        .expect("no R8Unorm (luma) texture");
    let luma_px = luma.pixels::<u8>().expect("u8 luma pixels");
    assert!(!luma_px.is_empty());
    assert!(
        luma_px.iter().all(|&v| v == 128),
        "luma plane should read 128"
    );

    // Chroma plane: RG8Unorm, Cb=100/Cr=150.
    let chroma = textures
        .iter()
        .find(|t| t.format() == MTLPixelFormat::RG8Unorm)
        .expect("no RG8Unorm (chroma) texture");
    let chroma_px = chroma.pixels::<[u8; 2]>().expect("[u8;2] chroma pixels");
    assert!(!chroma_px.is_empty());
    assert!(
        chroma_px.iter().all(|&[cb, cr]| cb == 100 && cr == 150),
        "chroma plane should read Cb=100/Cr=150"
    );
}
