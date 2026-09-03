//! Tests whether our (unentitled) process can open the out-of-process transport
//! that GTMTLReplayClient_preferDevice needs. RESULT (2026-09-01): it CRASHES -
//! createNewTransport SIGSEGVs inside its own apr_pool_create_ex, even though
//! this process's own apr_pool_create_ex (the sanity check below) works. So a
//! precondition createNewTransport needs is not satisfiable from our code. The
//! transport path itself is healthy (gpudebug drives it fine) but gpudebug is
//! Apple-signed with the entitlement; we are not. See dossier 06. This probe is
//! kept as the reproduction; it is EXPECTED to crash at createNewTransport.
//!
//! Usage (ALWAYS via probes/run.sh): probes/run.sh transport

use gputools_replay_sys::{client::ClientBuffer, ffi};
use objc2_foundation::NSRunLoop;
use probes::guard;
use std::process::ExitCode;

fn field_30(client: *const u8) -> usize {
    // SAFETY: `client` is a live ClientBuffer >= 0x138 bytes; 0x30 is in bounds
    // and pointer-aligned.
    unsafe { client.add(0x30).cast::<usize>().read() }
}

fn main() -> ExitCode {
    // SAFETY: single-threaded at the first line of main.
    unsafe { guard::set_unlock_env() };

    // SAFETY: apr bootstrap, then a zeroed client buffer for createNewTransport
    // to initialise (it calls GTMTLReplayClient_init on it internally).
    let mut backing = Box::new(ClientBuffer::new_zeroed());
    let client = backing.as_client_ptr();
    let client_bytes = client.cast::<u8>();
    unsafe {
        let rc = ffi::apr_initialize();
        if rc != 0 {
            eprintln!("transport FAILED: apr_initialize {rc}");
            return ExitCode::FAILURE;
        }
        // Prove apr works in THIS process before blaming createNewTransport:
        // the same apr_pool_create_ex call Session::open makes.
        let mut pool: *mut gputools_replay_sys::client::AprPool = std::ptr::null_mut();
        let rc = ffi::apr_pool_create_ex(
            &mut pool,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        println!("our own apr_pool_create_ex: rc={rc}, pool={:?}", pool);
    }
    println!(
        "client @ 0x{:x}; [client+0x30] before = 0x{:x}",
        client as usize,
        field_30(client_bytes)
    );
    println!("calling createNewTransport(client) ...");

    // SAFETY: 1 arg, the client (established). It opens the XPC connection.
    let ret = unsafe { ffi::GTMTLReplayClient_createNewTransport(client) };
    println!("createNewTransport returned 0x{:x}", ret as usize);
    println!(
        "[client+0x30] immediately after = 0x{:x}",
        field_30(client_bytes)
    );

    // The connection handshake is async; pump the run loop briefly and re-check.
    for i in 0..20 {
        // SAFETY: NSRunLoop main-loop pump; standard Cocoa run-loop usage.
        unsafe {
            let rl = NSRunLoop::currentRunLoop();
            let until = objc2_foundation::NSDate::dateWithTimeIntervalSinceNow(0.1);
            let _: bool = objc2::msg_send![&rl, runMode: objc2_foundation::NSDefaultRunLoopMode, beforeDate: &*until];
        }
        let v = field_30(client_bytes);
        if v != 0 {
            println!(
                "[client+0x30] POPULATED after {} run-loop pump(s): 0x{v:x}",
                i + 1
            );
            println!(
                "transport OK: the out-of-process path IS reachable; preferDevice is now runnable"
            );
            return ExitCode::SUCCESS;
        }
    }
    println!("[client+0x30] still 0 after pumping the run loop for ~2s");
    println!(
        "transport: the XPC connection did not populate the client - path not reachable unentitled"
    );
    ExitCode::SUCCESS
}
