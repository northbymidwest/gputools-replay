//! Live: needs the framework + captures/known-buffers.gputrace (fixture-apps/known-buffers.m).
//!
//! Build + capture (if not already present):
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/known-buffers \
//!         fixture-apps/known-buffers.m -framework Metal -framework Foundation
//!   fixture-apps/capture-late.sh /tmp/known-buffers captures/known-buffers.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay-hl --test live_hl_buffers -- --ignored
//!
//! Its own test binary (separate process): the substrate allows one session
//! per process, and each live_hl_* file is its own binary.

use gputools_replay_hl::{Capture, ReplayerConfig};

fn bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/known-buffers.gputrace"
    ))
    .to_owned()
}

#[test]
#[ignore = "live: needs the framework and captures/known-buffers.gputrace"]
fn buffers_heaps_pipelines_via_domain_api() {
    // Default config: the fixture's resources are used in-capture, so they
    // answer without forcing unused-resource loads.
    // SAFETY: single-threaded test process, before any other thread.
    unsafe { Capture::configure_env(&ReplayerConfig::default()) };
    let cap = Capture::open(&bundle()).expect("open known-buffers capture");
    let refs: Vec<u64> = (0..=64).collect();

    // in_a: the 256-byte ramp buffer, [i] = i, byte-exact.
    let bufs = cap.buffers(refs.clone()).expect("fetch buffers");
    let in_a = bufs
        .iter()
        .find(|b| b.raw_bytes().len() == 256 && b.as_slice::<u32>().unwrap()[0] == 0)
        .expect("in_a (256 B, ramp from 0)");
    assert_eq!(in_a.as_slice::<u32>().unwrap()[..4], [0, 1, 2, 3]);

    // Heap: the full 64 KiB backing store.
    let heaps = cap.heaps(refs.clone()).expect("fetch heaps");
    assert_eq!(heaps[0].raw_bytes().len(), 64 * 1024);

    // Pipeline: the compute pipeline's compiled Mach-O stage.
    let pipes = cap
        .pipeline_binaries(refs)
        .expect("fetch pipeline binaries");
    let stages = pipes[0].stages().expect("parse stages");
    assert!(
        stages
            .iter()
            .any(|s| s.mach_o.starts_with(&[0xcf, 0xfa, 0xed, 0xfe])
                || s.mach_o.starts_with(&[0xfe, 0xed, 0xfa, 0xcf])),
        "expected a Mach-O compiled stage"
    );
}
