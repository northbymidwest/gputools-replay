//! Live: needs the framework + captures/known-textures-late.gputrace.
//! Exercises the object-map enumeration (Capture::loaded_textures /
//! loaded_texture_refs / unused_resource_refs) against known ground truth:
//! without force-load, three textures are loaded (streamRefs 2/3/4) and four
//! are unused (6/7/8/9). No fetch and no ref sweep.
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay-hl --test live_hl_enumerate -- --ignored
//!
//! Its own test binary (separate process).

use gputools_replay_hl::Capture;
use std::collections::BTreeMap;

fn bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/known-textures-late.gputrace"
    ))
    .to_owned()
}

#[test]
#[ignore = "live: needs the framework and captures/known-textures-late.gputrace"]
fn enumerate_loaded_textures() {
    let cap = Capture::open(&bundle()).expect("open known-textures capture");

    // The three textures the captured commands touch are loaded; enumeration
    // finds them without a ref sweep.
    let refs = cap.loaded_texture_refs().expect("loaded_texture_refs");
    assert_eq!(refs, vec![2, 3, 4], "the loaded texture streamRefs, sorted");

    // loaded_textures pairs each with its descriptor in one pass.
    let textures = cap.loaded_textures().expect("loaded_textures");
    let got_refs: Vec<u64> = textures.iter().map(|(r, _)| *r).collect();
    assert_eq!(
        got_refs, refs,
        "loaded_textures refs match loaded_texture_refs"
    );

    for (r, d) in &textures {
        assert_eq!(d.stream_ref, *r);
        assert_eq!(d.pixel_format, 80, "ref {r}: BGRA8Unorm");
        assert_eq!(d.texture_type, 2, "ref {r}: 2D");
    }
    let dims: BTreeMap<u64, (u32, u32)> = textures
        .iter()
        .map(|(r, d)| (*r, (d.width, d.height)))
        .collect();
    assert_eq!(dims[&2], (64, 64));
    assert_eq!(dims[&3], (80, 64));
    assert_eq!(dims[&4], (112, 112));

    // The four unused textures are reported as unused (any-kind refs).
    let unused = cap.unused_resource_refs().expect("unused_resource_refs");
    for u in [6u64, 7, 8, 9] {
        assert!(
            unused.contains(&u),
            "streamRef {u} should be unused, got {unused:?}"
        );
    }
    assert!(
        !unused.contains(&2),
        "a loaded ref must not appear as unused"
    );
}
