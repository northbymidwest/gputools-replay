//! Live acceleration-structure fetch test against a ground-truth fixture.
//!
//! Requires the framework and `captures/accel-structure.gputrace`, generated
//! from `fixture-apps/accel-structure.m` (default triangle):
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/accel-structure \
//!         fixture-apps/accel-structure.m -framework Metal -framework Foundation
//!   fixture-apps/capture-late.sh /tmp/accel-structure captures/accel-structure.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay --test live_accel -- --ignored
//!
//! Its own test binary (separate process), one session.

use gputools_replay::Session;
use gputools_replay::config::ReplayerConfig;
use gputools_replay::reply::{AccelRecord, Reply};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(60);

fn bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/accel-structure.gputrace"
    ))
    .to_owned()
}

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

#[test]
#[ignore = "live: needs the framework and captures/accel-structure.gputrace"]
fn fetch_acceleration_structure_ground_truth() {
    // SAFETY: single-threaded test process, before any other thread.
    unsafe { Session::configure_env(&ReplayerConfig::default()) };
    let sess = Session::open(&bundle()).expect("open accel-structure capture");

    let refs: Vec<u64> = (0..=40).collect();
    let reply: Reply<AccelRecord> = sess
        .fetch_acceleration_structures(&refs, TIMEOUT)
        .expect("fetch_acceleration_structures");
    let recs = reply.records();
    assert_eq!(recs.len(), 1, "one bottom-level acceleration structure");

    let payload = reply.payload(&recs[0]);
    assert_eq!(payload.len(), 1816, "the primitive AS is 1816 bytes");

    // Byte format decoded in dossier 05, for the fixture's default triangle
    // (0,0,0), (1,0,0), (0,1,0).
    assert_eq!(u64_at(payload, 0x08), 1816, "0x08 = total size");

    // 0x0a0: geometry AABB, six contiguous floats (min xyz, max xyz).
    let aabb: Vec<f32> = (0..6).map(|i| f32_at(payload, 0x0a0 + i * 4)).collect();
    assert_eq!(
        aabb,
        [0.0, 0.0, 0.0, 1.0, 1.0, 0.0],
        "triangle bounding box"
    );

    // 0x418: the triangle vertices, verbatim input, tightly-packed float3.
    let verts: Vec<f32> = (0..9).map(|i| f32_at(payload, 0x418 + i * 4)).collect();
    assert_eq!(
        verts,
        [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        "the fixture's default triangle, preserved exactly"
    );
}
