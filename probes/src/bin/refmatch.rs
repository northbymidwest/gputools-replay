//! What is a reply record's field 0x00 keyed on, before and after playback?
//!
//! `replydump` established that the post-playback shift is real, not a parse
//! artifact: the raw info bytes say `eb 07 00 00` (2027), and field 0x08
//! is unchanged (24..27) across playback, so these are the SAME four resources
//! reporting a different value at 0x00. The open question is what that value
//! means and, crucially, whether the fetch still MATCHES on the original refs
//! (in which case coverage is unchanged and the number is informational) or on
//! the new ones (in which case a sweep bounded at 2000 can no longer name
//! them).
//!
//! Full sweeps cannot answer that, because the requested range always contains
//! the baseline refs. So this issues explicit, targeted request lists instead.
//! One session per process, so every experiment runs in this one process.
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh refmatch [path-to.gputrace]

use probes::{guard, reply, session};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(300);

fn fetch(sess: &session::Session, refs: &[u64], label: &str) -> Result<(), String> {
    let requests: Vec<session::FetchRequest> = refs
        .iter()
        .map(|&stream_ref| session::FetchRequest {
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

    let shown: String = if refs.len() <= 8 {
        format!("{refs:?}")
    } else {
        format!(
            "[{}..={}] ({} refs)",
            refs[0],
            refs[refs.len() - 1],
            refs.len()
        )
    };
    println!("  request {shown}");
    if records.is_empty() {
        println!("    -> 0 records   [{label}]");
        return Ok(());
    }
    let pairs: Vec<String> = records
        .iter()
        .map(|r| format!("streamRef={} ordinal={}", r.stream_ref, r.request_ordinal))
        .collect();
    println!(
        "    -> {} records: {}   [{label}]",
        records.len(),
        pairs.join(", ")
    );
    Ok(())
}

fn main() -> ExitCode {
    // SAFETY: process is single-threaded at the first line of main.
    unsafe { guard::set_unlock_env() };
    let bundle: PathBuf = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "captures/small.gputrace".to_owned())
        .into();
    match run(&bundle) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("refmatch FAILED: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(bundle: &std::path::Path) -> Result<(), String> {
    let sess = session::Session::open(bundle).map_err(|e| e.to_string())?;

    println!("BEFORE playback (index {}):", sess.command_index());
    // Single-ref requests: if 0x08 echoes the requested ref, each of these
    // answers with exactly that value, and a ref outside the set answers with
    // nothing. This is what decides which field is the stream ref.
    fetch(&sess, &[24], "single ref 24")?;
    fetch(&sess, &[26], "single ref 26")?;
    fetch(&sess, &[27], "single ref 27")?;
    fetch(&sess, &[99], "single ref 99, expect empty")?;
    fetch(&sess, &[25, 26, 27, 28], "the known-good four")?;
    fetch(&sess, &[2027, 2028, 2029, 2030], "control: should be empty")?;
    fetch(&sess, &(0..=2000).collect::<Vec<u64>>(), "full sweep")?;

    sess.play_all();
    println!("AFTER playback (index {}):", sess.command_index());

    // The discriminator: if the original refs still answer, the fetch matches
    // on them and the 0x00 value is informational. If only the shifted refs
    // answer, a sweep must reach past the shift to name these resources.
    fetch(
        &sess,
        &[25, 26, 27, 28],
        "do the ORIGINAL refs still answer?",
    )?;
    fetch(
        &sess,
        &[2027, 2028, 2029, 2030],
        "do the SHIFTED refs answer?",
    )?;
    fetch(
        &sess,
        &[29, 30, 31, 32],
        "control: neighbours, expect empty",
    )?;
    fetch(
        &sess,
        &(0..=2000).collect::<Vec<u64>>(),
        "full sweep, bound 2000",
    )?;
    fetch(
        &sess,
        &(0..=100).collect::<Vec<u64>>(),
        "narrow sweep, bound 100",
    )?;
    Ok(())
}
