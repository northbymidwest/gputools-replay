//! Live: needs the framework + captures/accel-structure.gputrace
//! (fixture-apps/accel-structure.m, the default triangle).
//!
//! Build + capture (if not already present):
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/accel-structure \
//!         fixture-apps/accel-structure.m -framework Metal -framework Foundation
//!   fixture-apps/capture-late.sh /tmp/accel-structure captures/accel-structure.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay-hl --test live_hl_accel -- --ignored
//!
//! Its own test binary (separate process).

use gputools_replay_hl::{Aabb, Capture, ReplayerConfig};

fn bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/accel-structure.gputrace"
    ))
    .to_owned()
}

#[test]
#[ignore = "live: needs the framework and captures/accel-structure.gputrace"]
fn acceleration_structure_via_domain_api() {
    // SAFETY: single-threaded test process, before any other thread.
    unsafe { Capture::configure_env(&ReplayerConfig::default()) };
    let cap = Capture::open(&bundle()).expect("open accel-structure capture");

    let refs: Vec<u64> = (0..=40).collect();
    let structures = cap
        .acceleration_structures(refs)
        .expect("fetch acceleration structures");
    assert_eq!(
        structures.len(),
        1,
        "one bottom-level acceleration structure"
    );

    let structure = &structures[0];
    // `triangles()` is bounded to exactly the header's triangle count field
    // (0x2c) - MEASURED 1 for this fixture (dossier 05) - so it returns
    // exactly that one triangle's 3 vertices, not the phantom trailing
    // vertices v1 read past it (61 more, reinterpreting other structure
    // sections). Checking `.len()` (not just the vertex values) is the
    // actual regression test for the phantom-vertex fix: a wrong/absent
    // count bound would silently pass the value check below while still
    // over-reading.
    let triangles = structure.triangles().expect("decode triangles");
    assert_eq!(
        triangles.len(),
        3,
        "exactly one triangle's three vertices (header count field == 1), no phantom trailing vertices"
    );
    assert_eq!(
        triangles,
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        "the fixture's default triangle, preserved exactly"
    );

    let aabb = structure.aabb().expect("decode aabb");
    assert_eq!(
        aabb,
        Aabb {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 1.0, 0.0],
        }
    );
}
