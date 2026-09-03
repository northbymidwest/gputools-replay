//! Live wireframe (dispatch-keyed) fetch test against a render fixture.
//!
//! Requires the framework and `captures/known-draws.gputrace`, generated from
//! `fixture-apps/known-draws.m`:
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/known-draws \
//!         fixture-apps/known-draws.m -framework Metal -framework Foundation
//!   fixture-apps/capture-late.sh /tmp/known-draws captures/known-draws.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay --test live_draws -- --ignored
//!
//! Its own test binary (separate process), one session.
//!
//! Note: wireframe fetch renders a wireframe image per draw. Requesting a
//! dispatchUID that is not a real draw makes the replayer raise an internal
//! error (`generateWireframeTexture`), which the safe crate surfaces as
//! `FetchError::Replayer` (it has no tolerate-errors escape hatch). So this
//! test requests only the draw indices the fixture actually produces (4..=8).

use gputools_replay::Session;
use gputools_replay::config::ReplayerConfig;
use gputools_replay::reply::{Reply, WireframeRecord};
use gputools_replay::request::{DispatchUid, WireframeRequest};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(60);

fn bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/known-draws.gputrace"
    ))
    .to_owned()
}

#[test]
#[ignore = "live: needs the framework and captures/known-draws.gputrace"]
fn fetch_wireframes_ground_truth() {
    // SAFETY: single-threaded test process, before any other thread.
    unsafe { Session::configure_env(&ReplayerConfig::default()) };
    let sess = Session::open(&bundle()).expect("open known-draws capture");

    // The fixture's three draws, rendered twice (phase 1 + phase 2), land at
    // dispatchUIDs 4..=8 in the command stream.
    let reqs: Vec<WireframeRequest> = (4u64..=8)
        .map(|uid| WireframeRequest {
            dispatch_uid: DispatchUid(uid),
            solid: false,
        })
        .collect();

    let reply: Reply<WireframeRecord> = sess
        .fetch_wireframes(&reqs, TIMEOUT)
        .expect("fetch_wireframes");
    let recs = reply.records();
    assert_eq!(recs.len(), 5, "five command-stream draws answered");

    // The answered dispatchUIDs are exactly the ones requested, each a
    // 256x256 R8 wireframe image (streamRef -1, dispatch-keyed).
    let mut uids: Vec<u32> = recs.iter().map(|r| r.dispatch_uid).collect();
    uids.sort_unstable();
    assert_eq!(uids, [4, 5, 6, 7, 8]);
    for r in recs {
        assert_eq!((r.width, r.height), (256, 256), "the 256x256 render target");
        assert_eq!(r.pixel_format, 10, "R8Unorm wireframe image");
        assert_eq!(reply.payload(r).len(), 256 * 256, "one byte per pixel");
    }

    // The fixture issues three distinct triangle draws; not every command-
    // stream index is one of them (render-pass overhead renders blank), so at
    // least the three triangles must produce a non-blank wireframe.
    let non_blank = recs
        .iter()
        .filter(|r| reply.payload(r).iter().any(|&px| px != 0))
        .count();
    assert!(
        non_blank >= 3,
        "at least the three triangle draws are non-blank (got {non_blank})"
    );
}
