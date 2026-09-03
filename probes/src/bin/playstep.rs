//! Verifies that a single playback step really advances the replay.
//!
//! This is the regression probe for the controller-pointer defect described in
//! `docs/findings/01-playback.md`. Playback was long believed to be blocked
//! in-process behind a GPU fence; it was not. The pointer being passed to
//! `playTo` was `GTMTLReplayController_init`'s discarded return value, which
//! points into the framework's read-only `__AUTH_CONST` segment. `playTo`
//! faulted on its first controller dereference, GPUToolsReplay's own
//! `HandleCrashSignal` caught the fault and then faulted again, and the
//! resulting signal-handler recursion pegged a core forever - which read from
//! the outside exactly like a spinning fence wait.
//!
//! The controller is field 1 of the `GTMTLReplayClient` struct
//! (`ClientBuffer::controller`). With it, `play_to` returns in well under a
//! second and the command index advances.
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh playstep [path-to.gputrace] [target-index]
//! Defaults: captures/small.gputrace, 1.
//!
//! Exits non-zero if the index does not reach the target, so this is a real
//! check and not just a print.

use probes::{guard, session};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    // SAFETY: process is single-threaded at the first line of main.
    unsafe { guard::set_unlock_env() };

    let mut args = std::env::args().skip(1);
    let bundle: PathBuf = args
        .next()
        .unwrap_or_else(|| "captures/small.gputrace".to_owned())
        .into();
    let target: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    // Optional third arg "rewind": also exercise GTMTLReplayController_rewind,
    // the one playback symbol with no live evidence at all. Gated so the
    // default regression check stays unaffected if rewind misbehaves.
    let try_rewind = args.next().is_some_and(|a| a == "rewind");

    let sess = match session::Session::open(&bundle) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("playstep FAILED to open session: {e}");
            return ExitCode::FAILURE;
        }
    };

    let controller = sess.controller_in_client();
    if controller.is_null() {
        eprintln!("playstep FAILED: client field 1 (controller) is null after load");
        return ExitCode::FAILURE;
    }

    // Printed before the call, and flushed, so that if this ever regresses to
    // a hang the operator still has the addresses to attach to.
    println!("pid            {}", std::process::id());
    println!("client         0x{:x}", sess.client_addr());
    println!("controller     0x{:x}", controller as usize);
    let before = sess.command_index();
    println!("index before   {before}");
    println!("play_to({target}) ...");
    let _ = std::io::stdout().flush();

    sess.play_to(target);

    let after = sess.command_index();
    println!("index after    {after}");

    if after < target {
        eprintln!("playstep FAILED: index {after} did not reach target {target}");
        return ExitCode::FAILURE;
    }
    if after == before {
        eprintln!("playstep FAILED: index did not advance from {before}");
        return ExitCode::FAILURE;
    }
    println!("playstep OK: advanced {before} -> {after}");

    if try_rewind {
        // Semantics are INFERRED from control flow only (teardown + restore
        // initial state, dossier 01). If that is right the command index
        // returns to its initial value; this is the first live evidence
        // either way.
        println!("calling rewind() ...");
        let _ = std::io::stdout().flush();
        sess.rewind();
        let rewound = sess.command_index();
        println!("index after rewind {rewound}");
        if rewound == 0 {
            println!("rewind OK: index returned to 0 (consistent with restore-initial-state)");
        } else {
            println!(
                "rewind NOTE: index is {rewound}, not 0 -- semantics differ from the inference"
            );
        }
    }

    ExitCode::SUCCESS
}
