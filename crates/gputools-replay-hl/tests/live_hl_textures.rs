//! Live: needs the framework + captures/known-textures-late.gputrace
//! (fixture-apps/known-textures.m, force-loaded since it's never sampled).
//!
//! Build + capture (if not already present):
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/known-textures \
//!         fixture-apps/known-textures.m -framework Metal -framework Foundation
//!   fixture-apps/capture-late.sh /tmp/known-textures captures/known-textures-late.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay-hl --test live_hl_textures -- --ignored
//!
//! Its own test binary (separate process).

use gputools_replay_hl::{Capture, MTLPixelFormat, ReplayerConfig, Texture};

fn bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/known-textures-late.gputrace"
    ))
    .to_owned()
}

/// Assert every pixel in the `cw` x `ch` top-left region of `t` equals
/// `bgra`, honouring the row stride via `rows()` (a wider destination
/// texture's untouched region is undefined and not checked).
fn assert_solid_region(t: &Texture, cw: usize, ch: usize, bgra: [u8; 4]) {
    for (y, row) in t.rows::<[u8; 4]>().expect("rows").enumerate().take(ch) {
        for (x, &px) in row.iter().enumerate().take(cw) {
            assert_eq!(
                px,
                bgra,
                "texture w={} pixel ({x},{y}) should be {bgra:02x?}",
                t.width()
            );
        }
    }
}

#[test]
#[ignore = "live: needs the framework and captures/known-textures-late.gputrace"]
fn textures_via_domain_api() {
    // The fixture's textures are unused (never read by captured commands), so
    // they must be force-loaded to answer (dossier 00).
    // SAFETY: single-threaded test process, before any other thread.
    unsafe {
        Capture::configure_env(&ReplayerConfig {
            force_load_unused_resources: true,
            ..ReplayerConfig::default()
        })
    };
    let cap = Capture::open(&bundle()).expect("open known-textures capture");

    let textures = cap.textures(0..=200).expect("fetch textures");

    // The two stored-content textures (a blit source and its destination) are
    // filled with cyan; ground truth is keyed by width: blit_src (w=64) is
    // fully cyan, blit_dst (w=80) has only the 64x64 blit region written.
    const CYAN_BGRA: [u8; 4] = [0x00, 0xff, 0xff, 0xff];
    for (width, cw, ch) in [(64u32, 64usize, 64usize), (80, 64, 64)] {
        let t = textures
            .iter()
            .find(|t| t.width() == width)
            .unwrap_or_else(|| panic!("no texture of width {width}"));
        assert_eq!(t.format(), MTLPixelFormat::BGRA8Unorm);
        assert_solid_region(t, cw, ch, CYAN_BGRA);
    }
}
