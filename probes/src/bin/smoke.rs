//! End-to-end smoke test of the ported substrate: open a capture, fetch every
//! texture in a streamRef sweep at natural size, parse the reply, print what
//! came back. Validates bootstrap -> load -> fetch -> parse against the real
//! framework.
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh smoke [path-to.gputrace] [max-stream-ref]
//! Defaults: captures/small.gputrace, 2000.

use probes::{guard, reply, session};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

fn main() -> ExitCode {
    // FIRST statement: single-threaded here, so setting the env is sound.
    // SAFETY: process is single-threaded at the first line of main.
    unsafe { guard::set_unlock_env() };

    let mut args = std::env::args().skip(1);
    let bundle: PathBuf = args
        .next()
        .unwrap_or_else(|| "captures/small.gputrace".to_owned())
        .into();
    let max_ref: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(2000);

    match run(&bundle, max_ref) {
        Ok(n) => {
            eprintln!("smoke OK: {n} records parsed from {}", bundle.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("smoke FAILED: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(bundle: &std::path::Path, max_ref: u64) -> Result<usize, String> {
    let sess = session::Session::open(bundle).map_err(|e| e.to_string())?;

    // Natural size (width=0, height=0) so textures come back unresampled.
    let requests: Vec<session::FetchRequest> = (0..=max_ref)
        .map(|stream_ref| session::FetchRequest {
            stream_ref,
            width: 0,
            height: 0,
            plane: 0,
        })
        .collect();

    let bytes = sess
        .fetch_textures(&requests, Duration::from_secs(300))
        .map_err(|e| e.to_string())?;

    let reply = reply::parse_reply(&bytes).map_err(|e| e.to_string())?;
    let records = reply::parse_records(&reply.info).map_err(|e| e.to_string())?;

    if records.is_empty() {
        return Err("the sweep returned zero records; the substrate did not work".to_owned());
    }

    let mut histogram: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    for r in &records {
        *histogram.entry(r.pixel_format).or_default() += 1;
    }
    eprintln!("pixel-format histogram (MTLPixelFormat -> count): {histogram:?}");

    Ok(records.len())
}
