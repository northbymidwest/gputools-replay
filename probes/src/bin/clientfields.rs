//! Read the client fields GTMTLReplayClient_preferDevice dereferences, WITHOUT
//! calling it, to see which is unpopulated on the in-process client.
//!
//! Backlog item 6. preferDevice (dossier 06) derefs [client], [client+0x30],
//! [client+0x60] (the last fed to _GTSMMTLContext_getObject) and faults live
//! on our initWithContext: client. This reads those offsets after a full load
//! to identify the null/absent one - a pure read, no fault.
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh clientfields [path-to.gputrace]

use probes::{guard, session};
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
            eprintln!("clientfields FAILED: {e}");
            return ExitCode::FAILURE;
        }
    };
    let base = sess.client_addr();
    println!("client base 0x{base:x}");
    // SAFETY: base is the live 312-byte ClientBuffer; 0x00/0x08/0x30/0x60 are
    // all within it (< 0x138) and pointer-aligned. Reading them as raw usize
    // is in-bounds and does not dereference the pointees.
    for off in [0x00usize, 0x08, 0x30, 0x60] {
        let v = unsafe { ((base + off) as *const usize).read() };
        println!("  [client+0x{off:02x}] = 0x{v:x}");
    }
    ExitCode::SUCCESS
}
