//! RE spike: what does playback do to the object map (finding #1), and why does
//! a post-playback batch fetch fail (finding #7)?
//!
//! Reports, on one session: the map object's ADDRESS and `resources` count
//! before vs after `play_all` (same object cleared, or a new map swapped in at
//! controller+0x8?), whether a known ref is still a texture after playback,
//! then a BATCH fetch of the pre-playback refs vs PER-REF retries, then the map
//! state again (does a fetch repopulate?).
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh playmap [path-to.gputrace] [force]
//! `force` sets MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE=1 before open.

use gputools_replay_sys::layout::controller_object_map;
use gputools_replay_sys::replay::GTMTLReplayObjectMap;
use probes::{guard, session};
use session::FetchRequest;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

fn map_addr(m: &GTMTLReplayObjectMap) -> usize {
    std::ptr::from_ref(m).addr()
}

/// (map object address, resources count, resource streamRefs sorted).
fn snapshot(sess: &session::Session) -> Option<(usize, usize, Vec<u64>)> {
    let map = unsafe { controller_object_map(sess.controller_in_client()) }.ok()?;
    let resources = map.resources();
    let keys = resources.allKeys();
    let mut refs: Vec<u64> = (0..keys.count())
        .map(|i| keys.objectAtIndex(i).unsignedLongLongValue())
        .collect();
    refs.sort_unstable();
    Some((map_addr(&map), resources.count(), refs))
}

fn natural(refs: &[u64]) -> Vec<FetchRequest> {
    refs.iter()
        .map(|&stream_ref| FetchRequest {
            stream_ref,
            width: 0,
            height: 0,
            plane: 0,
        })
        .collect()
}

fn main() -> ExitCode {
    // SAFETY: single-threaded at the first line of main.
    unsafe { guard::set_unlock_env() };
    let args: Vec<String> = std::env::args().collect();
    let bundle: PathBuf = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "captures/known-textures-late.gputrace".to_owned())
        .into();
    if args.get(2).map(String::as_str) == Some("force") {
        // SAFETY: still single-threaded, before the framework initialises.
        unsafe { std::env::set_var("MTLREPLAYER_FORCE_LOAD_UNUSED_RESOURCE", "1") };
        println!("(force-load enabled)");
    }
    let sess = match session::Session::open(&bundle) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("open failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let to = Duration::from_secs(30);

    let before = snapshot(&sess);
    println!("after open:      {before:?}");
    let refs = before
        .as_ref()
        .map(|(_, _, r)| r.clone())
        .unwrap_or_default();

    sess.play_all();
    let after = snapshot(&sess);
    println!("after play_all:  {after:?}");
    if let (Some((a1, _, _)), Some((a2, _, _))) = (&before, &after) {
        println!(
            "  map object: {}",
            if a1 == a2 {
                "SAME address (cleared in place)"
            } else {
                "DIFFERENT address (new map swapped in at controller+0x8)"
            }
        );
    }

    // Batch fetch of the pre-playback refs (finding #7), then per-ref retry.
    if !refs.is_empty() {
        match sess.fetch_textures(&natural(&refs), to) {
            Ok(b) => println!("batch fetch {refs:?}: OK ({} bytes)", b.len()),
            Err(e) => println!("batch fetch {refs:?}: ERR {e}"),
        }
        for &r in &refs {
            match sess.fetch_textures(&natural(&[r]), to) {
                Ok(b) => println!("  per-ref {r}: OK ({} bytes)", b.len()),
                Err(e) => println!("  per-ref {r}: ERR {e}"),
            }
        }
    }

    println!("after fetches:   {:?}", snapshot(&sess));
    ExitCode::SUCCESS
}
