//! Establish GT_ENV as the framework's global config table (backlog item 9).
//!
//! GT_ENV is a DATA export. The residency analysis put the config word at
//! GT_ENV+0x30, populated by GTMTLReplayController_init from MTLREPLAYER_*
//! environment variables (one bit per var). This bootstraps just far enough to
//! call init (apr + a pool, no session/load), reads the config word before and
//! after, and shows a chosen env var flips its bit - behaviour-confirming that
//! GT_ENV is the config global and how it is fed.
//!
//! Bit 11 (0x800) = MTLREPLAYER_FORCE_RESOURCES_RESIDENT (dossier 01).
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh gtenv

use gputools_replay_sys::ffi;
use probes::guard;
use std::process::ExitCode;

const CONFIG_WORD_OFFSET: usize = 0x30;
const BIT_FORCE_RESIDENT: u64 = 1 << 11; // MTLREPLAYER_FORCE_RESOURCES_RESIDENT

fn read_config_word() -> u64 {
    // SAFETY: GT_ENV is the framework's global config table; the config word is
    // a u64 at +0x30 (dossier 06). Reading it is an aligned, in-bounds load of
    // a live global.
    unsafe {
        core::ptr::addr_of!(ffi::GT_ENV)
            .cast::<u8>()
            .add(CONFIG_WORD_OFFSET)
            .cast::<u64>()
            .read()
    }
}

fn main() -> ExitCode {
    // Set the env var BEFORE init reads it. SAFETY: single-threaded here.
    unsafe {
        guard::set_unlock_env();
        std::env::set_var("MTLREPLAYER_FORCE_RESOURCES_RESIDENT", "1");
    }
    let addr = core::ptr::addr_of!(ffi::GT_ENV) as usize;
    println!("GT_ENV @ 0x{addr:x}; config word @ +0x{CONFIG_WORD_OFFSET:x}");

    let before = read_config_word();
    println!("config word before init: 0x{before:x}");

    // Minimal bootstrap: apr + a pool, then the config initialiser. No session,
    // no load, no replayer - init only populates the config from the env.
    // SAFETY: the exact sequence GPUToolsReplayService and Session::open use;
    // each call's arity is established (dossier 01 / HANDOFF).
    unsafe {
        let rc = ffi::apr_initialize();
        if rc != 0 {
            eprintln!("gtenv FAILED: apr_initialize rc {rc}");
            return ExitCode::FAILURE;
        }
        let mut pool: *mut gputools_replay_sys::client::AprPool = std::ptr::null_mut();
        let rc = ffi::apr_pool_create_ex(
            &mut pool,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        if rc != 0 || pool.is_null() {
            eprintln!("gtenv FAILED: apr_pool_create_ex rc {rc}");
            return ExitCode::FAILURE;
        }
        // Return value deliberately discarded (it is not a controller).
        let _ = ffi::GTMTLReplayController_init(pool);
    }

    let after = read_config_word();
    println!("config word after init:  0x{after:x}");
    let bit_set = after & BIT_FORCE_RESIDENT != 0;
    println!(
        "bit 11 (FORCE_RESOURCES_RESIDENT) after init: {}",
        if bit_set { "SET" } else { "clear" }
    );

    if bit_set {
        println!(
            "gtenv OK: GT_ENV is the config table; init set bit 11 from \
             MTLREPLAYER_FORCE_RESOURCES_RESIDENT=1"
        );
        ExitCode::SUCCESS
    } else {
        eprintln!("gtenv FAILED: bit 11 not set - GT_ENV/offset/bit assumption is wrong");
        ExitCode::FAILURE
    }
}
