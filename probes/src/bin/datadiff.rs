//! Per-streamRef: does `play_all()` change a texture's fetched payload bytes?
//!
//! The coverage probe shows the record SET is unchanged by playback; `replydump`
//! shows the whole `data` blob differs. This splits that: for each streamRef
//! answered both before and after playback, it compares that record's payload
//! bytes exactly. The point is to tell whether playback alters USED resources
//! (which would generalise the fixture's blit_dst degradation to real captures)
//! or only transient/drawable ones.
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh datadiff [path-to.gputrace] [max-stream-ref]
//! Defaults: captures/small.gputrace, 2000.

use probes::{guard, reply, session};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(300);

/// One fetched record's payload plus the geometry needed to describe a change.
struct Payload {
    bytes: Vec<u8>,
    width: u32,
    height: u16,
    format: u32,
}

/// streamRef -> payload for one sweep.
fn sweep(sess: &session::Session, max_ref: u64) -> Result<BTreeMap<u32, Payload>, String> {
    let requests: Vec<session::FetchRequest> = (0..=max_ref)
        .map(|stream_ref| session::FetchRequest {
            stream_ref,
            width: 0,
            height: 0,
            plane: 0,
        })
        .collect();
    let bytes = sess
        .fetch_textures(&requests, TIMEOUT)
        .map_err(|e| e.to_string())?;
    let parsed = reply::parse_reply(&bytes).map_err(|e| e.to_string())?;
    let records = reply::parse_records(&parsed.info).map_err(|e| e.to_string())?;
    let mut out = BTreeMap::new();
    for r in &records {
        let (off, size) = (r.data_offset as usize, r.size as usize);
        let bytes = parsed.data.get(off..off + size).unwrap_or(&[]).to_vec();
        out.insert(
            r.stream_ref,
            Payload {
                bytes,
                width: r.width,
                height: r.height,
                format: r.pixel_format,
            },
        );
    }
    Ok(out)
}

fn main() -> ExitCode {
    // SAFETY: process is single-threaded at the first line of main.
    unsafe { guard::set_unlock_env() };
    let mut args = std::env::args().skip(1);
    let bundle: PathBuf = args
        .next()
        .unwrap_or_else(|| "captures/small.gputrace".to_owned())
        .into();
    let max_ref: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(2000);

    let sess = match session::Session::open(&bundle) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("datadiff FAILED to open session: {e}");
            return ExitCode::FAILURE;
        }
    };
    let before = match sweep(&sess, max_ref) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("datadiff FAILED (before): {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "before: {} records; play_all() index {} ->",
        before.len(),
        sess.command_index()
    );
    sess.play_all();
    eprintln!(
        "                                  index {}",
        sess.command_index()
    );
    let after = match sweep(&sess, max_ref) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("datadiff FAILED (after): {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut changed = 0;
    let mut same = 0;
    for (r, before_p) in &before {
        let b = &before_p.bytes;
        match after.get(r) {
            Some(after_p) if &after_p.bytes == b => same += 1,
            Some(after_p) => {
                changed += 1;
                let a = &after_p.bytes;
                // How much changed, and whether "after" collapsed to one value.
                let diffs = a.iter().zip(b).filter(|(x, y)| x != y).count();
                let mut hist: BTreeMap<[u8; 4], usize> = BTreeMap::new();
                for px in a.as_chunks::<4>().0 {
                    *hist.entry(*px).or_default() += 1;
                }
                let (dom, n) = hist
                    .iter()
                    .max_by_key(|(_, n)| **n)
                    .map(|(k, n)| (*k, *n))
                    .unwrap_or(([0; 4], 0));
                let share = if a.is_empty() {
                    0.0
                } else {
                    n as f64 / (a.len() / 4) as f64 * 100.0
                };
                eprintln!(
                    "  CHANGED ref {r:<5} {}x{} fmt {}: {diffs}/{} bytes differ; after dominant {:02x}{:02x}{:02x}{:02x} ({:.1}%)",
                    before_p.width,
                    before_p.height,
                    before_p.format,
                    b.len(),
                    dom[0],
                    dom[1],
                    dom[2],
                    dom[3],
                    share
                );
            }
            None => eprintln!("  DROPPED ref {r:<5} answered before, not after"),
        }
    }
    eprintln!(
        "summary: {same} unchanged, {changed} changed across playback, of {} refs answered both",
        same + changed
    );
    ExitCode::SUCCESS
}
