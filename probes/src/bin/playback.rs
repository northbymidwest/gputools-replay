//! Coverage-gap probe: does driving playback change which streamRefs answer?
//!
//! Tests the hypothesis in docs/findings/00-texture-fetch.md and
//! docs/findings/01-playback.md: some streamRefs may not be fetchable until
//! playback has advanced past the command that creates or writes them. This
//! runs a baseline natural-size sweep, drives playback via the controller,
//! then sweeps again and diffs the sets of streamRefs that answered.
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh playback [path-to.gputrace] [max-stream-ref] [mode]
//! Defaults: captures/small.gputrace, 2000, playall.
//! mode is "playall" or "playto:N".

use probes::{guard, reply, session};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

/// How the probe drives playback between the two fetch sweeps.
enum Mode {
    PlayAll,
    PlayTo(u32),
}

impl Mode {
    fn parse(s: &str) -> Result<Self, String> {
        if s == "playall" {
            return Ok(Mode::PlayAll);
        }
        if let Some(rest) = s.strip_prefix("playto:") {
            let index: u32 = rest
                .parse()
                .map_err(|_| format!("playto target {rest:?} is not a u32"))?;
            return Ok(Mode::PlayTo(index));
        }
        Err(format!(
            "unknown mode {s:?}; expected \"playall\" or \"playto:N\""
        ))
    }
}

/// Bound for both fetch sweeps. Matches `smoke`'s natural-size sweep: 44 ms
/// at 64x64 and 439 ms moving 119 MB at natural size on `small.gputrace`, so
/// this is headroom, not an expected wait.
const TIMEOUT: Duration = Duration::from_secs(300);

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
    let mode_arg = args.next().unwrap_or_else(|| "playall".to_owned());
    let mode = match Mode::parse(&mode_arg) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("playback FAILED: {e}");
            return ExitCode::FAILURE;
        }
    };

    match run(&bundle, max_ref, &mode) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("playback FAILED: {e}");
            ExitCode::FAILURE
        }
    }
}

/// One natural-size (width=0, height=0) sweep of `0..=max_ref`, returning the
/// set of streamRefs that answered. Natural size, as in `smoke`, so the reply
/// carries the real image rather than resampled pixels.
fn sweep(sess: &session::Session, max_ref: u64) -> Result<BTreeSet<u32>, String> {
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

    let reply = reply::parse_reply(&bytes).map_err(|e| e.to_string())?;
    let records = reply::parse_records(&reply.info).map_err(|e| e.to_string())?;

    // The STREAM REF (field 0x08), not field 0x00. Field 0x00 is a per-session
    // request ordinal that climbs on every fetch, so diffing it made playback
    // look like it swapped one set of resources for another when the same four
    // answer throughout. See `reply::InfoRecord`.
    Ok(records.into_iter().map(|r| r.stream_ref).collect())
}

fn run(bundle: &std::path::Path, max_ref: u64, mode: &Mode) -> Result<(), String> {
    let sess = session::Session::open(bundle).map_err(|e| e.to_string())?;

    let before = sweep(&sess, max_ref)?;
    eprintln!(
        "baseline: {} of {} streamRefs answered",
        before.len(),
        max_ref + 1
    );

    match mode {
        Mode::PlayAll => {
            eprintln!("driving playback: play_all()");
            sess.play_all();
        }
        Mode::PlayTo(index) => {
            eprintln!("driving playback: play_to({index})");
            sess.play_to(*index);
        }
    }

    let after = sweep(&sess, max_ref)?;
    eprintln!(
        "after playback: {} of {} streamRefs answered",
        after.len(),
        max_ref + 1
    );

    let newly_answering: Vec<&u32> = after.difference(&before).collect();
    let dropped: Vec<&u32> = before.difference(&after).collect();

    eprintln!(
        "diff: {} newly answering after playback, {} dropped after playback",
        newly_answering.len(),
        dropped.len()
    );
    if !newly_answering.is_empty() {
        eprintln!("newly answering streamRefs: {newly_answering:?}");
    }
    if !dropped.is_empty() {
        eprintln!("dropped streamRefs: {dropped:?}");
    }

    Ok(())
}
