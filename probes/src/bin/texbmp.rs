//! Fetch one texture and write it as a BMP (and raw), before and after
//! play_all(), for the oracle pixel comparison against gpudebug's own fetch.
//!
//! gpudebug (Apple's tool) can fetch a real texture and export a PNG; our path
//! fetches the same texture as raw BGRA8Unorm. This writes ours in a form that
//! is both viewable and byte-comparable, so "playback preserves used content"
//! (datadiff, dossier 01) can be upgraded to "our fetched pixels match Apple's".
//!
//! Writes <out>/<label>-before.bmp/.bgra and -after.bmp/.bgra. BMP is 32-bit
//! bottom-up BGRA, which is nearly a straight copy of the fetched bytes.
//!
//! Usage (ALWAYS via probes/run.sh):
//!   probes/run.sh texbmp [path-to.gputrace] [stream-ref] [out-dir]
//! Defaults: captures/small.gputrace, 24, /tmp.

use probes::{guard, reply, session};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(300);

/// Fetch exactly `stream_ref` at natural size; return (bytes, w, h, bpr, fmt).
fn fetch_one(
    sess: &session::Session,
    stream_ref: u64,
) -> Result<(Vec<u8>, u32, u32, u32, u32), String> {
    let req = session::FetchRequest {
        stream_ref,
        width: 0,
        height: 0,
        plane: 0,
    };
    let bytes = sess
        .fetch_textures(&[req], TIMEOUT)
        .map_err(|e| e.to_string())?;
    let parsed = reply::parse_reply(&bytes).map_err(|e| e.to_string())?;
    let records = reply::parse_records(&parsed.info).map_err(|e| e.to_string())?;
    let r = records
        .first()
        .ok_or_else(|| format!("streamRef {stream_ref} answered no record"))?;
    let (off, size) = (r.data_offset as usize, r.size as usize);
    let payload = parsed
        .data
        .get(off..off + size)
        .ok_or("payload out of range")?
        .to_vec();
    Ok((
        payload,
        r.width,
        r.height as u32,
        r.bytes_per_row,
        r.pixel_format,
    ))
}

/// Write a 32-bit BGRA BMP (bottom-up) from top-down BGRA rows.
fn write_bmp(path: &Path, px: &[u8], w: u32, h: u32, bpr: u32) -> Result<(), String> {
    let (w, h, bpr) = (w as usize, h as usize, bpr as usize);
    let row_bytes = w * 4;
    let img_size = row_bytes * h;
    let file_size = 54 + img_size;
    let mut out = Vec::with_capacity(file_size);
    // BITMAPFILEHEADER
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    // BITMAPINFOHEADER
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes()); // positive => bottom-up
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(img_size as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    // Pixels: last source row first.
    for y in (0..h).rev() {
        let start = y * bpr;
        let row = px.get(start..start + row_bytes).ok_or("row out of range")?;
        out.extend_from_slice(row);
    }
    std::fs::write(path, &out).map_err(|e| e.to_string())
}

fn dump(sess: &session::Session, r: u64, out: &Path, phase: &str) -> Result<(), String> {
    let (px, w, h, bpr, fmt) = fetch_one(sess, r)?;
    // Tightly repack (drop any row padding) for the .bgra so a byte compare is
    // pure pixels; the BMP writer handles bpr itself.
    let mut packed = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h as usize {
        let s = y * bpr as usize;
        packed.extend_from_slice(&px[s..s + w as usize * 4]);
    }
    let base = out.join(format!("ref{r}-{phase}"));
    std::fs::write(base.with_extension("bgra"), &packed).map_err(|e| e.to_string())?;
    write_bmp(&base.with_extension("bmp"), &px, w, h, bpr)?;
    println!(
        "  {phase}: ref {r} {w}x{h} fmt {fmt} bpr {bpr} -> {} ({} px bytes)",
        base.with_extension("bmp").display(),
        packed.len()
    );
    let _ = std::io::stdout().flush();
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
    let stream_ref: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(24);
    let out: PathBuf = args.next().unwrap_or_else(|| "/tmp".to_owned()).into();

    let sess = match session::Session::open(&bundle) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("texbmp FAILED to open session: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = dump(&sess, stream_ref, &out, "before") {
        eprintln!("texbmp FAILED (before): {e}");
        return ExitCode::FAILURE;
    }
    println!("play_all() ... index {} ->", sess.command_index());
    sess.play_all();
    println!("                index {}", sess.command_index());
    if let Err(e) = dump(&sess, stream_ref, &out, "after") {
        eprintln!("texbmp FAILED (after): {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
