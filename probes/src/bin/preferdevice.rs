//! Audit GTMTLReplayClient_preferDevice: call it live, on a correct, loaded
//! client, and see whether it returns cleanly.
//!
//! Backlog item 6. Signature is established (dossier 06): one argument, the
//! client, called AFTER load: (it dereferences client fields load: populates).
//! The two prior segfaults (HANDOFF 3) are attributed to calling too early /
//! on the wrong object. This opens a full session (init + load) and only then
//! calls it, with an optional MTLREPLAYER_OVERRIDE_DEVICE_REGISTRY_ID.
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh preferdevice [path-to.gputrace]

use probes::{guard, session};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    // SAFETY: single-threaded at the first line of main.
    unsafe { guard::set_unlock_env() };
    let bundle: PathBuf = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "captures/small.gputrace".to_owned())
        .into();

    let sess = match session::Session::open(&bundle) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("preferdevice FAILED to open session: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Printed before the call: if it crashes, this is the last thing seen and
    // `sample`/the exit signal tells us it faulted rather than returned.
    println!("session loaded; client 0x{:x}", sess.client_addr());
    if let Some(v) = std::env::var_os("MTLREPLAYER_OVERRIDE_DEVICE_REGISTRY_ID") {
        println!("MTLREPLAYER_OVERRIDE_DEVICE_REGISTRY_ID = {v:?}");
    }
    println!("calling GTMTLReplayClient_preferDevice(client) ...");
    let _ = std::io::stdout().flush();

    sess.prefer_device();

    // Reaching here is the result: it returned rather than crashing.
    println!("prefer_device RETURNED cleanly");
    // Confirms the client is still usable (device still bound) after the call.
    println!(
        "client still live after prefer_device (command index {})",
        sess.command_index()
    );
    ExitCode::SUCCESS
}
