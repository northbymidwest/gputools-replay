//! Fetch a batch of a non-texture request class by streamRef and dump the raw
//! reply. Probe support for the fetch/decode classes graduated by shape
//! (dossiers 03/05): what does a GTReplayFetchPipelineBinaries /
//! GTReplayFetchAccelerationStructure reply actually look like?
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh rawfetch <ClassName> [path-to.gputrace] [max-stream-ref] [out]
//! e.g. probes/run.sh rawfetch GTReplayFetchPipelineBinaries \
//!        captures/corpus.gputrace 2000 /tmp

use probes::{guard, session};
use std::ffi::CString;
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
    let class = match args.next() {
        Some(c) => c,
        None => {
            eprintln!("usage: rawfetch <ClassName> [trace] [max-ref] [out]");
            return ExitCode::FAILURE;
        }
    };
    let bundle: PathBuf = args
        .next()
        .unwrap_or_else(|| "captures/corpus.gputrace".to_owned())
        .into();
    let refs_arg = args.next().unwrap_or_else(|| "2000".to_owned());
    let out: PathBuf = args.next().unwrap_or_else(|| "/tmp".to_owned()).into();
    let refs: Vec<u64> = if refs_arg.contains(',') {
        refs_arg
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect()
    } else {
        (0..=refs_arg.parse().unwrap_or(2000)).collect()
    };

    let sess = match session::Session::open(&bundle) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rawfetch FAILED to open: {e}");
            return ExitCode::FAILURE;
        }
    };
    let cname = CString::new(class.clone()).unwrap();
    match sess.fetch_raw(&cname, &refs, TIMEOUT) {
        Ok(bytes) => {
            let path = out.join(format!("{class}.reply.bin"));
            let _ = std::fs::write(&path, &bytes);
            println!("{class}: reply {} bytes -> {}", bytes.len(), path.display());
            // First bytes, to spot a bplist / NSKeyedArchiver header vs raw.
            let head = &bytes[..bytes.len().min(32)];
            let hex: Vec<String> = head.iter().map(|b| format!("{b:02x}")).collect();
            let ascii: String = head
                .iter()
                .map(|&b| {
                    if (0x20..0x7f).contains(&b) {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            println!("  head hex:   {}", hex.join(" "));
            println!("  head ascii: {ascii}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{class}: fetch_raw returned error: {e}");
            ExitCode::FAILURE
        }
    }
}
