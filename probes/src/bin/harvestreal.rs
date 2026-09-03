//! Validate the harvester getters against a REAL "capture" block and decode the
//! 0x30-byte plane descriptor. The MTLTexture-* content files in a capture
//! bundle ARE harvester blocks (magic "capture"); this reads one, hands it to
//! the framework's getters, and prints the decoded descriptor.
//!
//! Usage (ALWAYS via probes/run.sh - though no replayer session is used):
//!   probes/run.sh harvestreal <path-to-MTLTexture-file>

use gputools_replay_sys::ffi;
use std::process::ExitCode;

fn main() -> ExitCode {
    let path = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("usage: harvestreal <MTLTexture-file>");
            return ExitCode::FAILURE;
        }
    };
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("harvestreal: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let block = data.as_ptr().cast::<core::ffi::c_void>();
    let size = data.len();

    // SAFETY: `block`/`size` describe a real "capture" block (the MTLTexture
    // file). The getters validate the magic themselves and only compute offsets
    // within `[block, block+size)`.
    unsafe {
        let meta = ffi::GTHarvesterGetMetadata(block, size);
        if meta.is_null() {
            eprintln!("harvestreal: GetMetadata rejected the file (not a capture block?)");
            return ExitCode::FAILURE;
        }
        let planes = ffi::GTHarvesterGetTexturePlaneCount(block);
        println!("valid capture block, {planes} plane(s)");
        for i in 0..planes {
            let p = ffi::GTHarvesterGetTexturePlane(block, i).cast::<u64>();
            // 0x30-byte descriptor = six u64: fmt, width, height, depth, bpr, size
            let f = |n: usize| p.add(n).read();
            println!(
                "  plane{i}: pixelFormat={} width={} height={} depth={} bytesPerRow={} bytesPerImage={}",
                f(0),
                f(1),
                f(2),
                f(3),
                f(4),
                f(5)
            );
        }
        let payload = ffi::GTHarvesterGetData(block, size).cast::<u8>();
        let meta_len = payload as usize - block as usize;
        println!("data payload starts at block+0x{meta_len:x} (metadataSize)");
    }
    ExitCode::SUCCESS
}
