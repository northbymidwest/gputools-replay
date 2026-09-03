//! Live: needs the framework + captures/known-stencil.gputrace. One test.
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay-hl --test live_hl_described_stencil -- --ignored
use gputools_replay_hl::{Capture, ManifestStatus, ReplayerConfig};
use std::path::PathBuf;

fn cap(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../captures")).join(name)
}

#[test]
#[ignore = "live: needs the framework and captures"]
fn combined_depth_stencil_aspects_are_unattributed() {
    // SAFETY: single-threaded test process, before any other thread.
    unsafe {
        Capture::configure_env(&ReplayerConfig {
            force_load_unused_resources: true,
            ..ReplayerConfig::default()
        })
    };
    let c = Capture::open(&cap("known-stencil.gputrace")).expect("open");
    assert_eq!(
        c.manifest_status(),
        ManifestStatus::Ok(5),
        "known-stencil's manifest describes 5 textures"
    );

    let texs = c.textures(0..=60).expect("textures");
    // The combined Depth32Float_Stencil8 (260) descriptors are transparent to
    // the join (their aspects fetch under 252/261), never an error.
    let described = c.describe(&texs);

    // Every attributed descriptor exact-matches its texture's format+dims.
    for (t, d) in texs.iter().zip(&described.per_texture) {
        if let Some(desc) = d {
            assert_eq!(
                desc.format,
                t.format().0 as u32,
                "attributed descriptor exact-matches format"
            );
            assert_eq!(desc.width, t.width());
            assert_eq!(desc.height, t.height());
        }
    }
    // The combined-DS aspect fetches (depth 252 / stencil 261) are unattributed.
    let aspect_unattributed = texs.iter().zip(&described.per_texture).any(|(t, d)| {
        let f = t.format().0 as u32;
        (f == 252 || f == 261) && d.is_none()
    });
    assert!(
        aspect_unattributed,
        "combined depth-stencil aspects should be unattributed (None)"
    );
    // Colour / base-stencil textures DO attribute.
    assert!(
        described.per_texture.iter().any(Option::is_some),
        "colour/base-stencil should attribute"
    );
    // Every single-plane manifest descriptor placed - nothing left over.
    assert!(
        described.unplaced.is_empty(),
        "all single-plane descriptors should be placed, got unplaced: {:?}",
        described.unplaced
    );
}
