//! Live buffer + heap fetch tests against a ground-truth fixture capture.
//!
//! Requires the framework and `captures/known-buffers.gputrace`, generated
//! from `fixture-apps/known-buffers.m`:
//!   clang -fobjc-arc -fmodules -O0 -o /tmp/known-buffers \
//!         fixture-apps/known-buffers.m -framework Metal -framework Foundation
//!   fixture-apps/capture-late.sh /tmp/known-buffers captures/known-buffers.gputrace
//!
//! Run with:
//!   MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0 \
//!     cargo test -p gputools-replay --test live_buffers -- --ignored
//!
//! Its own test binary (separate process) so its `Session::open` does not
//! collide with the other live suites' one-session-per-process guard.

use gputools_replay::Session;
use gputools_replay::config::ReplayerConfig;
use gputools_replay::reply::{BufferRecord, HeapRecord, PipelineRecord, Reply};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(60);

fn sample_bundle() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../captures/known-buffers.gputrace"
    ))
    .to_owned()
}

/// Read a record's payload as little-endian `u32`s.
fn as_u32s<T: gputools_replay::reply::Record>(reply: &Reply<T>, r: &T) -> Vec<u32> {
    reply
        .payload(r)
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect()
}

#[test]
#[ignore = "live: needs the framework and captures/known-buffers.gputrace"]
fn fetch_buffers_heaps_and_pipelines() {
    // Default config: the fixture's resources are used in-capture, so they
    // answer without forcing unused-resource loads.
    // SAFETY: single-threaded test process, before any other thread.
    unsafe { Session::configure_env(&ReplayerConfig::default()) };
    let sess = Session::open(&sample_bundle()).expect("open known-buffers capture");

    // A superset of the fixture's streamRefs; the reply holds only the buffers.
    let refs: Vec<u64> = (0..=64).collect();

    // --- Buffers: four resources with exact byte ground truth. ---
    let reply: Reply<BufferRecord> = sess.fetch_buffers(&refs, TIMEOUT).expect("fetch_buffers");
    let recs = reply.records();
    assert_eq!(recs.len(), 4, "expected in_a, in_b, out, heap_buf");

    // in_a: 256 bytes, [i] = i.
    let in_a = recs
        .iter()
        .find(|r| r.size == 256 && as_u32s(&reply, r)[0] == 0)
        .expect("in_a (256 B, ramp from 0)");
    assert_eq!(
        as_u32s(&reply, in_a)[..4],
        [0, 1, 2, 3],
        "in_a[i] should equal i"
    );

    // heap_buf: 256 bytes, [i] = 0x2000 + i (same size as in_a, distinct start).
    let heap_buf = recs
        .iter()
        .find(|r| r.size == 256 && as_u32s(&reply, r)[0] == 0x2000)
        .expect("heap_buf (256 B, ramp from 0x2000)");
    assert_eq!(as_u32s(&reply, heap_buf)[..3], [0x2000, 0x2001, 0x2002]);

    // in_b: 384 bytes, [i] = 0x1000 + i.
    let in_b = recs.iter().find(|r| r.size == 384).expect("in_b (384 B)");
    let b = as_u32s(&reply, in_b);
    assert!(b.iter().enumerate().all(|(i, &v)| v == 0x1000 + i as u32));

    // out: 512 bytes; out[i<64] = 0x3000 + 4i, out[64..96] = 0x1000 + i.
    let out = recs.iter().find(|r| r.size == 512).expect("out (512 B)");
    let o = as_u32s(&reply, out);
    assert_eq!(o[10], 0x3000 + 4 * 10, "out[10] = 0x3000 + 4*10");
    assert_eq!(o[70], 0x1000 + 70, "out[70] = 0x1000 + 70");
    assert_eq!(o[100], 0, "out[100] = 0 (no input contributes)");

    // --- Heap: one resource; the 64 KB backing store holds heap_buf's bytes. ---
    let hreply: Reply<HeapRecord> = sess.fetch_heaps(&refs, TIMEOUT).expect("fetch_heaps");
    assert_eq!(hreply.records().len(), 1, "one heap");
    let heap = &hreply.records()[0];
    let bytes = hreply.payload(heap);
    assert_eq!(bytes.len(), 64 * 1024, "the full heap backing store");
    // heap_buf (0x2000, 0x2001, ...) is sub-allocated somewhere in the heap.
    let needle: Vec<u8> = (0x2000u32..0x2000 + 8)
        .flat_map(|v| v.to_le_bytes())
        .collect();
    assert!(
        bytes.windows(needle.len()).any(|w| w == needle),
        "heap payload should contain heap_buf's 0x2000.. pattern"
    );

    // --- Pipelines: the compute pipeline the kernel needs, as a nested
    // bplist of the compiled Mach-O binary + stats (dossier 03). ---
    let preply: Reply<PipelineRecord> = sess
        .fetch_pipeline_binaries(&refs, TIMEOUT)
        .expect("fetch_pipeline_binaries");
    assert!(
        !preply.records().is_empty(),
        "the compute pipeline should answer"
    );
    let pipe = &preply.records()[0];
    assert_ne!(pipe.handle, 0, "a real 64-bit pipeline handle");
    let payload = preply.payload(pipe);
    assert_eq!(payload.len(), pipe.size as usize);
    assert!(
        payload.starts_with(b"bplist00"),
        "pipeline payload is a nested bplist"
    );
    assert!(
        payload
            .windows(4)
            .any(|w| w == [0xcf, 0xfa, 0xed, 0xfe] || w == [0xfe, 0xed, 0xfa, 0xcf]),
        "pipeline payload contains a Mach-O binary"
    );
}
