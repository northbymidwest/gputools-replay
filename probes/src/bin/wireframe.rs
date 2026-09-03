//! Probe the dispatch-keyed GTReplayFetchWireframe: sweep dispatchUID values
//! and report which (if any) answer. Item 11: dispatch-keyed fetches need a
//! valid dispatchUID (an 8-byte union) from the command stream; this searches
//! for one by value.
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh wireframe [trace] [max-uid]

use probes::{guard, reply, session};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(300);

fn main() -> ExitCode {
    // SAFETY: single-threaded at the first line of main.
    unsafe {
        guard::set_unlock_env();
        if std::env::var_os("PROBE_TOLERATE_REPLAYER_ERRORS").is_none() {
            std::env::set_var("PROBE_TOLERATE_REPLAYER_ERRORS", "1");
        }
    }
    let mut args = std::env::args().skip(1);
    let bundle: PathBuf = args
        .next()
        .unwrap_or_else(|| "captures/corpus.gputrace".to_owned())
        .into();
    let max_uid: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(500);

    let sess = match session::Session::open(&bundle) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("wireframe FAILED to open: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Batch the whole sweep in one fetch; the reply's records tell us which
    // dispatchUIDs produced wireframe data.
    let uids: Vec<u64> = (0..=max_uid).collect();
    match sess.fetch_wireframe(&uids, TIMEOUT) {
        Ok(bytes) => {
            let _ = std::fs::write("/tmp/wireframe.reply.bin", &bytes);
            println!("reply {} bytes -> /tmp/wireframe.reply.bin", bytes.len());
            match reply::parse_reply(&bytes) {
                Ok(r) => {
                    let recs = reply::parse_records(&r.info)
                        .map(|v| v.len())
                        .unwrap_or(usize::MAX);
                    println!(
                        "  unknown_count {} info {}B ({} rec) data {}B",
                        r.unknown_count,
                        r.info.len(),
                        if recs == usize::MAX { 0 } else { recs },
                        r.data.len()
                    );
                    if !r.data.is_empty() || !r.info.is_empty() {
                        println!("  WIREFRAME DATA RETURNED");
                    }
                }
                Err(e) => println!("  reply parse: {e}"),
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("wireframe fetch error: {e}");
            ExitCode::FAILURE
        }
    }
}
