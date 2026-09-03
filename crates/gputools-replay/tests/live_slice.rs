//! Live slice-selection test: proves `TextureRequest.slice` steers
//! `GTReplayFetchTexture` to the corresponding array slice, and that the
//! reply record self-describes the plane/slice/level it answers
//! (`TextureRecord.plane`/`slice`/`level`, decoded from info fields
//! 0x48/0x4c - docs/findings/00-texture-fetch.md).
//!
//! Requires the framework and `captures/known-mips.gputrace`, generated from
//! `fixture-apps/known-mips.m`:
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/known-mips \
//!         fixture-apps/known-mips.m -framework Metal -framework Foundation
//!   fixture-apps/capture-late.sh /tmp/known-mips captures/known-mips.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay --test live_slice -- --ignored
//!
//! Its own test binary (separate process), one session.

use gputools_replay::Session;
use gputools_replay::config::ReplayerConfig;
use gputools_replay::reply::{Reply, TextureRecord};
use gputools_replay::request::TextureRequest;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(60);

fn bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/known-mips.gputrace"
    ))
    .to_owned()
}

fn first_pixel(reply: &Reply<TextureRecord>, r: &TextureRecord) -> [u8; 4] {
    let px = reply.payload(r);
    [px[0], px[1], px[2], px[3]]
}

#[test]
#[ignore = "live: needs the framework and captures/known-mips.gputrace"]
fn slice_selects_array_slice() {
    // known-mips' array texture is never sampled by a captured command, so it
    // must be force-loaded to answer (dossier 00). configure_env applies that
    // before open.
    // SAFETY: single-threaded test process, before any other thread.
    unsafe {
        Session::configure_env(&ReplayerConfig {
            force_load_unused_resources: true,
            ..ReplayerConfig::default()
        })
    };
    let sess = Session::open(&bundle()).expect("open known-mips capture");

    // Find the fixture's 64x64 array texture streamRef: sweep slice-0/level-0
    // requests (the default) and take the first record at the base size.
    // streamRefs are sparse (dossier 00), so sweep rather than guess an index.
    let sweep: Vec<TextureRequest> = (0u64..=40).map(TextureRequest::natural).collect();
    let sweep_reply: Reply<TextureRecord> = sess
        .fetch_textures(&sweep, TIMEOUT)
        .expect("sweep fetch_textures");
    let base = sweep_reply
        .records()
        .iter()
        .find(|r| r.width == 64 && r.height == 64)
        .expect("no 64x64 texture in known-mips sweep 0..=40");
    let stream_ref = u64::from(base.stream_ref);

    let slice0 = TextureRequest::natural(stream_ref).with_slice_level(0, 0);
    let reply0: Reply<TextureRecord> = sess
        .fetch_textures(&[slice0], TIMEOUT)
        .expect("fetch slice 0");
    let r0 = reply0.records().first().expect("slice 0 record");
    assert_eq!(
        first_pixel(&reply0, r0),
        [0, 0, 255, 255],
        "slice 0 should decode to red (BGRA)"
    );
    assert_eq!(r0.slice, 0, "record should self-describe slice 0");
    assert_eq!(r0.level, 0, "record should self-describe level 0");

    let slice1 = TextureRequest::natural(stream_ref).with_slice_level(1, 0);
    let reply1: Reply<TextureRecord> = sess
        .fetch_textures(&[slice1], TIMEOUT)
        .expect("fetch slice 1");
    let r1 = reply1.records().first().expect("slice 1 record");
    assert_eq!(
        first_pixel(&reply1, r1),
        [0, 255, 0, 255],
        "slice 1 should decode to green (BGRA)"
    );
    assert_eq!(r1.slice, 1, "record should self-describe slice 1");
    assert_eq!(r1.level, 0, "record should self-describe level 0");

    // A distinct level, same slice: the record should self-describe level 1
    // (mip halving makes this independently checkable via width too).
    let level1 = TextureRequest::natural(stream_ref).with_slice_level(0, 1);
    let reply_l1: Reply<TextureRecord> = sess
        .fetch_textures(&[level1], TIMEOUT)
        .expect("fetch level 1");
    let rl1 = reply_l1.records().first().expect("level 1 record");
    assert_eq!(rl1.slice, 0, "record should self-describe slice 0");
    assert_eq!(rl1.level, 1, "record should self-describe level 1");
    assert_eq!(rl1.width, 32, "level 1 should be the halved mip (64 -> 32)");

    // A non-default plane: the record should self-describe plane 1 (known-mips
    // is not a combined depth/stencil texture, so the content is not
    // meaningful here - only that the record echoes the requested plane).
    let plane1 = gputools_replay::request::Region::ZERO;
    let plane1_req = TextureRequest::new(stream_ref, plane1, 1);
    let reply_p1: Reply<TextureRecord> = sess
        .fetch_textures(&[plane1_req], TIMEOUT)
        .expect("fetch plane 1");
    let rp1 = reply_p1.records().first().expect("plane 1 record");
    assert_eq!(rp1.plane, 1, "record should self-describe plane 1");
}
