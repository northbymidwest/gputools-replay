//! Parsing of a GTReplayResponse payload.
//!
//! The payload is an NSKeyedArchiver binary plist whose root is a dictionary
//! with three keys: `unknown` (an array; see `Reply::unknown_count` for what
//! is and is not established about it), `info` (the descriptor table) and
//! `data` (concatenated raw pixels). We walk `$objects` by UID rather than
//! using a general unarchiver, because only these three keys are needed and
//! the archive is otherwise plain.

use plist::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Reply {
    /// The length of the `unknown` array's `NS.objects`. What this array
    /// actually holds is not established - it is NOT a count of unresolved
    /// requests; that reading was an unbacked guess. MEASURED: it is
    /// present and empty (`unknown_count == 0`) in every reply seen here,
    /// including `corpus.gputrace` swept 0..2000, where 182 of
    /// 2000 requested streamRefs answered and 1,818 did not
    /// (`splits_a_real_reply_into_unknown_info_and_data` below) - so an
    /// empty `unknown` array coexists with a large coverage gap, and this
    /// field does not explain it. No caller reads this field; it is kept
    /// only because the key's presence and array shape are a real
    /// self-consistency check on the reply (a reply genuinely missing the
    /// key is malformed).
    pub unknown_count: usize,
    pub info: Vec<u8>,
    pub data: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReplyError {
    #[error("payload is not a binary plist: {0}")]
    Plist(#[from] plist::Error),
    #[error("reply archive has an unexpected shape: {0}")]
    UnexpectedShape(String),
    #[error("info table is {len} bytes, not a multiple of the {stride}-byte record stride")]
    TrailingBytes { len: usize, stride: usize },
    #[error("record is too short to read the field at offset {offset:#x}")]
    ShortRecord { offset: usize },
}

fn shape(msg: impl Into<String>) -> ReplyError {
    ReplyError::UnexpectedShape(msg.into())
}

/// Stride of one info record. Confirmed two ways: the length of every real
/// `info` blob divides by 80 exactly, and the producer disassembled from
/// GPUToolsCompatService iterates its source records with a 0x50 stride.
pub const RECORD_LEN: usize = 80;

/// Offsets whose meaning is not established. Preserved verbatim so a later
/// capture that populates one is visible rather than silently dropped.
const UNMAPPED_OFFSETS: &[usize] = &[
    0x04, 0x0c, 0x10, 0x14, 0x20, 0x24, 0x28, 0x2c, 0x3c, 0x48, 0x4c,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoRecord {
    /// Field 0x00. **Not a stream ref**, despite what this parser called it
    /// until 2026-09-01. It is a per-session ordinal that counts requests as
    /// the replayer processes them.
    ///
    /// MEASURED (`probes/run.sh refmatch`, `small.gputrace`): on the FIRST
    /// fetch of a session it equals the request's 1-based position in the
    /// submitted list, so a `0..=2000` sweep answers stream ref 24 with 25 -
    /// which is precisely why `+1` looked like a plausible stream-ref
    /// encoding. Single-ref fetches of 24, 26 and 27 answer 1, 3 and 5, and
    /// the value keeps climbing across the session (2021, 2060, 4062 on later
    /// fetches of the SAME resources). It is strictly increasing within a
    /// reply.
    ///
    /// NOT established: the exact accounting. It advances by more than the
    /// number of requests submitted, so something else increments it too.
    /// Do NOT key on this field and do NOT compare it across fetches.
    pub request_ordinal: u32,
    /// Field 0x08: the stream ref this record answers, echoed back from the
    /// request.
    ///
    /// MEASURED (`probes/run.sh refmatch`): requesting exactly `[24]`, `[26]`
    /// or `[27]` answers with this field set to 24, 26 and 27 respectively;
    /// requesting `[99]` or `[29,30,31,32]` answers nothing. This is the
    /// stable resource identity across playback - it is unchanged by
    /// `play_all()` while [`InfoRecord::request_ordinal`] moves.
    ///
    /// Not unique on its own: a reply may answer one stream ref more than
    /// once (see `one_stream_ref_can_answer_more_than_once`). Full identity
    /// is (stream ref, plane).
    pub stream_ref: u32,
    pub data_offset: u32,
    pub size: u32,
    pub width: u32,
    pub height: u16,
    pub depth: u16,
    pub pixel_format: u32,
    pub bytes_per_row: u32,
    pub bytes_per_image: u32,
    pub unmapped: BTreeMap<&'static str, u32>,
}

fn u32_at(buf: &[u8], off: usize) -> Result<u32, ReplyError> {
    let end = off + 4;
    let slice = buf
        .get(off..end)
        .ok_or(ReplyError::ShortRecord { offset: off })?;
    let arr: [u8; 4] = slice
        .try_into()
        .map_err(|_| ReplyError::ShortRecord { offset: off })?;
    Ok(u32::from_le_bytes(arr))
}

fn u16_at(buf: &[u8], off: usize) -> Result<u16, ReplyError> {
    let end = off + 2;
    let slice = buf
        .get(off..end)
        .ok_or(ReplyError::ShortRecord { offset: off })?;
    let arr: [u8; 2] = slice
        .try_into()
        .map_err(|_| ReplyError::ShortRecord { offset: off })?;
    Ok(u16::from_le_bytes(arr))
}

pub fn parse_records(info: &[u8]) -> Result<Vec<InfoRecord>, ReplyError> {
    if !info.len().is_multiple_of(RECORD_LEN) {
        return Err(ReplyError::TrailingBytes {
            len: info.len(),
            stride: RECORD_LEN,
        });
    }
    let mut out = Vec::with_capacity(info.len() / RECORD_LEN);
    for chunk in info.as_chunks::<RECORD_LEN>().0 {
        let mut unmapped = BTreeMap::new();
        for &off in UNMAPPED_OFFSETS {
            let name: &'static str = match off {
                0x04 => "0x04",
                0x0c => "0x0c",
                0x10 => "0x10",
                0x14 => "0x14",
                0x20 => "0x20",
                0x24 => "0x24",
                0x28 => "0x28",
                0x2c => "0x2c",
                0x3c => "0x3c",
                0x48 => "0x48",
                _ => "0x4c",
            };
            unmapped.insert(name, u32_at(chunk, off)?);
        }
        out.push(InfoRecord {
            request_ordinal: u32_at(chunk, 0x00)?,
            stream_ref: u32_at(chunk, 0x08)?,
            data_offset: u32_at(chunk, 0x18)?,
            size: u32_at(chunk, 0x1c)?,
            width: u32_at(chunk, 0x30)?,
            height: u16_at(chunk, 0x34)?,
            depth: u16_at(chunk, 0x36)?,
            pixel_format: u32_at(chunk, 0x38)?,
            bytes_per_row: u32_at(chunk, 0x40)?,
            bytes_per_image: u32_at(chunk, 0x44)?,
            unmapped,
        });
    }
    Ok(out)
}

/// Resolve one `$objects` entry, following a UID indirection if present.
fn resolve<'a>(objects: &'a [Value], v: &'a Value) -> Result<&'a Value, ReplyError> {
    match v {
        Value::Uid(uid) => {
            let idx = usize::try_from(uid.get()).map_err(|_| shape("UID out of range"))?;
            objects
                .get(idx)
                .ok_or_else(|| shape(format!("UID {idx} out of bounds")))
        }
        other => Ok(other),
    }
}

pub fn parse_reply(bytes: &[u8]) -> Result<Reply, ReplyError> {
    let root: Value = plist::from_bytes(bytes)?;
    let dict = root
        .as_dictionary()
        .ok_or_else(|| shape("root is not a dictionary"))?;
    let objects = dict
        .get("$objects")
        .and_then(Value::as_array)
        .ok_or_else(|| shape("no $objects array"))?;

    // $top.root points at the payload dictionary.
    let top = dict
        .get("$top")
        .and_then(Value::as_dictionary)
        .ok_or_else(|| shape("no $top dictionary"))?;
    let root_ref = top.get("root").ok_or_else(|| shape("no $top.root"))?;
    let payload = resolve(objects, root_ref)?
        .as_dictionary()
        .ok_or_else(|| shape("$top.root is not a dictionary"))?;

    let keys = payload
        .get("NS.keys")
        .and_then(Value::as_array)
        .ok_or_else(|| shape("payload has no NS.keys"))?;
    let vals = payload
        .get("NS.objects")
        .and_then(Value::as_array)
        .ok_or_else(|| shape("payload has no NS.objects"))?;
    if keys.len() != vals.len() {
        return Err(shape("NS.keys and NS.objects differ in length"));
    }

    let mut unknown_count = None;
    let mut info = None;
    let mut data = None;
    for (k, v) in keys.iter().zip(vals.iter()) {
        let name = resolve(objects, k)?
            .as_string()
            .ok_or_else(|| shape("a payload key is not a string"))?
            .to_string();
        let value = resolve(objects, v)?;
        match name.as_str() {
            "unknown" => {
                let arr = value
                    .as_dictionary()
                    .and_then(|d| d.get("NS.objects"))
                    .and_then(Value::as_array)
                    .ok_or_else(|| shape("`unknown` is not an NSArray"))?;
                unknown_count = Some(arr.len());
            }
            "info" => {
                info = Some(
                    value
                        .as_data()
                        .ok_or_else(|| shape("`info` is not data"))?
                        .to_vec(),
                );
            }
            "data" => {
                data = Some(
                    value
                        .as_data()
                        .ok_or_else(|| shape("`data` is not data"))?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }

    Ok(Reply {
        unknown_count: unknown_count.ok_or_else(|| shape("missing key `unknown`"))?,
        info: info.ok_or_else(|| shape("missing key `info`"))?,
        data: data.ok_or_else(|| shape("missing key `data`"))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../fixtures/reply_corpus_64x64.plist"
        );
        std::fs::read(path).expect("fixture missing; see fixtures/README.md")
    }

    #[test]
    fn splits_a_real_reply_into_unknown_info_and_data() {
        let reply = parse_reply(&fixture()).unwrap();
        assert_eq!(reply.unknown_count, 0);
        assert_eq!(reply.info.len() % RECORD_LEN, 0);
        assert!(!reply.data.is_empty());
    }

    #[test]
    fn the_corpus_reply_has_182_records_with_the_known_format_mix() {
        let reply = parse_reply(&fixture()).unwrap();
        let records = parse_records(&reply.info).unwrap();
        assert_eq!(records.len(), 182);
        let count = |fmt: u32| records.iter().filter(|r| r.pixel_format == fmt).count();
        assert_eq!(count(10), 8, "R8Unorm");
        assert_eq!(count(70), 2, "RGBA8Unorm");
        assert_eq!(count(80), 162, "BGRA8Unorm");
        assert_eq!(count(125), 10, "RGBA32Float");
    }

    /// The stream ref is NOT unique per record (HANDOFF 2.5): the real reply
    /// has 182 records over 180 distinct values, so two refs each answer
    /// twice. Fetched-record identity is (streamRef, plane); this test keeps
    /// anyone from keying on the ref alone.
    #[test]
    fn one_stream_ref_can_answer_more_than_once() {
        let reply = parse_reply(&fixture()).unwrap();
        let records = parse_records(&reply.info).unwrap();
        let distinct: std::collections::BTreeSet<u32> =
            records.iter().map(|r| r.stream_ref).collect();
        assert_eq!(distinct.len(), 180);
    }

    /// The two fields are distinct and must not be confused again: within one
    /// reply the ordinal is strictly increasing, while the stream ref is not.
    /// Reading the ordinal as a stream ref is what made playback look like it
    /// changed fetch coverage when it does not.
    #[test]
    fn the_request_ordinal_is_not_the_stream_ref() {
        let reply = parse_reply(&fixture()).unwrap();
        let records = parse_records(&reply.info).unwrap();
        let ordinals: Vec<u32> = records.iter().map(|r| r.request_ordinal).collect();
        let refs: Vec<u32> = records.iter().map(|r| r.stream_ref).collect();
        assert_ne!(ordinals, refs);
        assert!(
            ordinals.windows(2).all(|w| w[1] > w[0]),
            "the ordinal is strictly increasing within a reply"
        );
        // In this fixture (one sweep of a fresh session) a ref's FIRST answer
        // carries the ordinal of its 1-based request position, i.e. ref + 1 -
        // which is exactly why field 0x00 read as a stream-ref encoding. The
        // two refs that answer twice have their repeat at a much higher
        // ordinal (1121 answers at 1122 and again at 1554), so the rule holds
        // per first occurrence, not per record.
        let mut seen = std::collections::BTreeSet::new();
        let firsts: Vec<(u32, u32)> = records
            .iter()
            .filter(|r| seen.insert(r.stream_ref))
            .map(|r| (r.request_ordinal, r.stream_ref))
            .collect();
        assert_eq!(firsts.len(), 180);
        assert!(
            firsts.iter().all(|(o, r)| *o == r + 1),
            "a ref's first answer is ordinaled at its request position"
        );
    }

    #[test]
    fn an_info_table_with_trailing_bytes_is_refused() {
        assert!(parse_records(&[0u8; RECORD_LEN + 1]).is_err());
    }
}
