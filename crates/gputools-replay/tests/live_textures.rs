//! Live texture fetch + playback tests against a ground-truth fixture.
//!
//! Requires the framework and `captures/known-textures-late.gputrace`,
//! generated from `fixture-apps/known-textures.m`:
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/known-textures \
//!         fixture-apps/known-textures.m -framework Metal -framework Foundation
//!   fixture-apps/capture-late.sh /tmp/known-textures captures/known-textures-late.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay --test live_textures -- --ignored
//!
//! Its own test binary (separate process), one session: fetch + playback share
//! the one session the process is allowed.

use gputools_replay::Session;
use gputools_replay::config::ReplayerConfig;
use gputools_replay::reply::{Reply, TextureRecord};
use gputools_replay::request::TextureRequest;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(60);

fn bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/known-textures-late.gputrace"
    ))
    .to_owned()
}

/// Assert every pixel in the `cw` x `ch` top-left region of a texture record
/// equals `bgra`, honouring the row stride (`bytes_per_row` may exceed
/// `width * 4`). The region is the part the fixture actually wrote; pixels
/// outside it (e.g. a blit destination larger than the blit) are undefined.
fn assert_solid(
    reply: &Reply<TextureRecord>,
    r: &TextureRecord,
    cw: usize,
    ch: usize,
    bgra: [u8; 4],
) {
    let px = reply.payload(r);
    let bpr = r.bytes_per_row as usize;
    for y in 0..ch {
        for x in 0..cw {
            let i = y * bpr + x * 4;
            assert_eq!(
                &px[i..i + 4],
                &bgra,
                "texture w={} pixel ({x},{y}) should be {bgra:02x?}",
                r.width
            );
        }
    }
}

#[test]
#[ignore = "live: needs the framework and captures/known-textures-late.gputrace"]
fn fetch_textures_and_playback() {
    // The fixture's textures are unused (never read by captured commands), so
    // they must be force-loaded to answer (dossier 00). configure_env applies
    // that before open. One session (one per process): fetch then playback.
    // SAFETY: single-threaded test process, before any other thread.
    unsafe {
        Session::configure_env(&ReplayerConfig {
            force_load_unused_resources: true,
            ..ReplayerConfig::default()
        })
    };
    let sess = Session::open(&bundle()).expect("open known-textures capture");

    let reqs: Vec<TextureRequest> = (0u64..=200).map(TextureRequest::natural).collect();
    let reply: Reply<TextureRecord> = sess.fetch_textures(&reqs, TIMEOUT).expect("fetch_textures");

    // The two stored-content textures (a blit source and its destination) are
    // filled with cyan and are byte-perfect before playback; the clear-only
    // rows are unused-resource placeholders (fixture artifact, dossier 00), so
    // ground truth is asserted on the stored pair, keyed by their widths.
    const CYAN_BGRA: [u8; 4] = [0x00, 0xff, 0xff, 0xff];
    // (width, checked region): blit_src (w=64) is fully cyan; blit_dst (w=80)
    // has only the 64x64 blit region written, the rest undefined.
    for (width, cw, ch) in [(64u32, 64usize, 64usize), (80, 64, 64)] {
        let r = reply
            .records()
            .iter()
            .find(|r| r.width == width)
            .unwrap_or_else(|| panic!("no texture of width {width}"));
        assert_eq!(r.pixel_format, 80, "BGRA8Unorm"); // MTLPixelFormatBGRA8Unorm
        assert_solid(&reply, r, cw, ch, CYAN_BGRA);
    }

    // Playback advances and rewinds the command index on the same session.
    assert_eq!(sess.command_index(), 0);
    sess.play_to(1);
    assert_eq!(sess.command_index(), 1);
    sess.rewind();
    assert_eq!(sess.command_index(), 0);
}
