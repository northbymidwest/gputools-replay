//! Live: needs the framework + captures/known-draws.gputrace
//! (fixture-apps/known-draws.m).
//!
//! Build + capture (if not already present):
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/known-draws \
//!         fixture-apps/known-draws.m -framework Metal -framework Foundation
//!   fixture-apps/capture-late.sh /tmp/known-draws captures/known-draws.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay-hl --test live_hl_draws -- --ignored
//!
//! Its own test binary (separate process).
//!
//! Note: wireframe fetch renders an image per draw; a dispatchUID that is
//! not a real draw makes the replayer raise an internal error, failing the
//! whole batch. So this requests only the draw indices the fixture actually
//! produces (4..=8).

use gputools_replay_hl::{Capture, ReplayerConfig};

fn bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/known-draws.gputrace"
    ))
    .to_owned()
}

#[test]
#[ignore = "live: needs the framework and captures/known-draws.gputrace"]
fn wireframes_via_domain_api() {
    // SAFETY: single-threaded test process, before any other thread.
    unsafe { Capture::configure_env(&ReplayerConfig::default()) };
    let cap = Capture::open(&bundle()).expect("open known-draws capture");

    let wireframes = cap.wireframes(4..=8, false).expect("fetch wireframes");
    assert_eq!(wireframes.len(), 5, "five command-stream draws answered");

    let mut non_blank = 0;
    for w in &wireframes {
        assert_eq!(
            (w.width(), w.height()),
            (256, 256),
            "the 256x256 render target"
        );
        let px = w.pixels::<u8>().expect("R8 pixels");
        assert_eq!(px.len(), 256 * 256, "one byte per pixel");
        if px.iter().any(|&v| v != 0) {
            non_blank += 1;
        }
    }
    assert!(
        non_blank >= 3,
        "at least the three triangle draws are non-blank (got {non_blank})"
    );
}
