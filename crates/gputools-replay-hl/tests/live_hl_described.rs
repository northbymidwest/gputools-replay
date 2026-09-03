//! Live: needs the framework + captures/known-ambiguous.gputrace. One test
//! (Session is a process-wide one-shot). Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay-hl --test live_hl_described -- --ignored
//! Its own test binary (separate process).
use gputools_replay_hl::{Capture, ReplayerConfig};
use std::path::PathBuf;

fn cap(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../captures")).join(name)
}

#[test]
#[ignore = "live: needs the framework and captures"]
fn intra_run_attribution_is_correct() {
    // SAFETY: single-threaded test process, before any other thread.
    unsafe {
        Capture::configure_env(&ReplayerConfig {
            force_load_unused_resources: true,
            ..ReplayerConfig::default()
        })
    };
    let c = Capture::open(&cap("known-ambiguous.gputrace")).expect("open");
    let texs = c.textures(0..=50).expect("textures");
    let described = c.describe(&texs);

    // known-ambiguous: three 64x64 BGRA textures; ground truth pins colour->mip,
    // so streamRef 2=red/mip1, 3=green/mip3, 4=blue/mip7. The same-dims run is
    // attributed by the measured ordinal order.
    let mip_of = |sr: u64| {
        texs.iter()
            .position(|t| t.stream_ref() == sr)
            .and_then(|i| described.per_texture[i])
            .map(|d| d.mip_levels)
    };
    assert_eq!(mip_of(2), Some(1), "streamRef 2 (red) is mip 1");
    assert_eq!(mip_of(3), Some(3), "streamRef 3 (green) is mip 3");
    assert_eq!(mip_of(4), Some(7), "streamRef 4 (blue) is mip 7");
}
