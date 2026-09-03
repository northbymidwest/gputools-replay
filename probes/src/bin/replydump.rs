//! Dumps the full fetch reply before and after playback, field by field.
//!
//! Investigating the post-playback coverage result: after `play_all()` the
//! same COUNT of streamRefs answers (4 on `small.gputrace`), but the reported
//! refs move to `max_ref + 27 ..= max_ref + 30` - `[2027..2030]` at a sweep
//! bound of 2000, `[6027..6030]` at 6000 - while the baseline is always
//! `[25,26,27,28]`. Refs that track the sweep bound look like a parse artifact,
//! but `parse_records` walks a fixed 80-byte stride and rejects a table whose
//! length is not a multiple of it, so a misaligned walk should fail loudly
//! rather than yield plausible numbers.
//!
//! So: dump the raw bytes and every parsed field, both sweeps, and let the
//! evidence decide instead of the hypothesis.
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh replydump [path-to.gputrace] [max-stream-ref] [out-dir]
//! Defaults: captures/small.gputrace, 2000, /tmp.

use probes::{guard, reply, session};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(300);

fn sweep_raw(sess: &session::Session, max_ref: u64) -> Result<Vec<u8>, String> {
    let requests: Vec<session::FetchRequest> = (0..=max_ref)
        .map(|stream_ref| session::FetchRequest {
            stream_ref,
            width: 0,
            height: 0,
            plane: 0,
        })
        .collect();
    sess.fetch_textures(&requests, TIMEOUT)
        .map_err(|e| e.to_string())
}

/// Prints every parsed field of every record, plus the raw first 16 bytes, so
/// a shifted or reinterpreted field is visible rather than inferred.
fn report(label: &str, bytes: &[u8], out_dir: &std::path::Path) -> Result<(), String> {
    let parsed = reply::parse_reply(bytes).map_err(|e| e.to_string())?;
    let records = reply::parse_records(&parsed.info).map_err(|e| e.to_string())?;

    println!("=== {label} ===");
    println!(
        "  reply {} bytes | info {} bytes ({} records of {}) | data {} bytes | unknown_count {}",
        bytes.len(),
        parsed.info.len(),
        parsed.info.len() / reply::RECORD_LEN,
        reply::RECORD_LEN,
        parsed.data.len(),
        parsed.unknown_count,
    );

    for (i, r) in records.iter().enumerate() {
        println!(
            "  [{i}] streamRef {:<8} ordinal {:<8} off {:<10} size {:<10} {}x{}x{} fmt {} bpr {} bpi {}",
            r.stream_ref,
            r.request_ordinal,
            r.data_offset,
            r.size,
            r.width,
            r.height,
            r.depth,
            r.pixel_format,
            r.bytes_per_row,
            r.bytes_per_image,
        );
        let un: Vec<String> = r
            .unmapped
            .iter()
            .filter(|&(_, &v)| v != 0)
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        if !un.is_empty() {
            println!("        nonzero unmapped: {}", un.join(" "));
        }
        // The raw record, so a field read at the wrong offset is visible.
        let start = i * reply::RECORD_LEN;
        let raw = &parsed.info[start..start + reply::RECORD_LEN];
        let hex: Vec<String> = raw[..16].iter().map(|b| format!("{b:02x}")).collect();
        println!("        raw[0x00..0x10]: {}", hex.join(" "));
    }

    let info_path = out_dir.join(format!("{label}.info.bin"));
    let data_path = out_dir.join(format!("{label}.data.bin"));
    std::fs::write(&info_path, &parsed.info).map_err(|e| e.to_string())?;
    std::fs::write(&data_path, &parsed.data).map_err(|e| e.to_string())?;
    println!(
        "  wrote {} and {}",
        info_path.display(),
        data_path.display()
    );
    Ok(())
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
    let out_dir: PathBuf = args.next().unwrap_or_else(|| "/tmp".to_owned()).into();

    match run(&bundle, max_ref, &out_dir) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("replydump FAILED: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(bundle: &std::path::Path, max_ref: u64, out_dir: &std::path::Path) -> Result<(), String> {
    let sess = session::Session::open(bundle).map_err(|e| e.to_string())?;
    println!("max_ref {max_ref} ({} requests)", max_ref + 1);

    let before = sweep_raw(&sess, max_ref)?;
    report("before", &before, out_dir)?;

    println!("index before play_all: {}", sess.command_index());
    sess.play_all();
    println!("index after  play_all: {}", sess.command_index());

    let after = sweep_raw(&sess, max_ref)?;
    report("after", &after, out_dir)?;

    println!(
        "raw replies identical: {}",
        if before == after { "YES" } else { "no" }
    );
    Ok(())
}
