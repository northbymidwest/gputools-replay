//! Reply decoding: the NSKeyedArchiver bplist envelope a fetch returns, and
//! the typed, owned records packed into its `info` table.
//!
//! The payload is an NSKeyedArchiver binary plist whose root is a dictionary
//! with three keys: `unknown` (an array; what it holds beyond "present and
//! empty" is not established), `info` (the descriptor table) and `data`
//! (concatenated raw payload bytes). We walk `$objects` by UID rather than
//! using a general unarchiver, because only these three keys are needed and
//! the archive is otherwise plain.

use crate::FetchError;
use crate::util::{u16_at, u32_at, u64_at};
use plist::Value;
use std::collections::BTreeMap;

/// The raw envelope of a decoded fetch reply: the unknown array's length and
/// the two data blobs (`info`, the descriptor table; `data`, the payload
/// bytes), before either is interpreted into typed records.
#[derive(Debug, Clone)]
pub struct RawReply {
    /// The length of the `unknown` array's `NS.objects`. What this array
    /// actually holds is not established - it is present and empty in every
    /// reply seen so far. Kept only because the key's presence and array
    /// shape are a self-consistency check on the reply.
    pub unknown_len: usize,
    /// The descriptor table: a sequence of fixed-stride records, one per
    /// fetched resource.
    pub info: Vec<u8>,
    /// Concatenated raw payload bytes, indexed into by each record's
    /// `data_range`.
    pub data: Vec<u8>,
}

fn shape(msg: impl Into<String>) -> FetchError {
    FetchError::Parse(msg.into())
}

/// Resolve one `$objects` entry, following a UID indirection if present.
fn resolve<'a>(objects: &'a [Value], v: &'a Value) -> Result<&'a Value, FetchError> {
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

impl RawReply {
    /// Parse the NSKeyedArchiver bplist envelope a fetch returns into its
    /// `unknown`/`info`/`data` parts.
    pub fn parse(bytes: &[u8]) -> Result<Self, FetchError> {
        let root: Value = plist::from_bytes(bytes).map_err(|e| shape(e.to_string()))?;
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

        let mut unknown_len = None;
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
                    unknown_len = Some(arr.len());
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

        Ok(RawReply {
            unknown_len: unknown_len.ok_or_else(|| shape("missing key `unknown`"))?,
            info: info.ok_or_else(|| shape("missing key `info`"))?,
            data: data.ok_or_else(|| shape("missing key `data`"))?,
        })
    }
}

/// A decoded 80-byte reply record. Each fetch class has its own layout.
pub trait Record: Sized {
    /// Record stride in the info table (80 for every class seen).
    const STRIDE: usize = 80;
    /// Decode one record from an 80-byte chunk.
    fn decode(chunk: &[u8]) -> Self;
    /// (offset, len) of this record's payload within the reply `data`.
    fn data_range(&self) -> (usize, usize);
}

// Named offsets within an 80-byte info record. Every fetch class shares the
// same physical stride; which slots a class interprets differs (dossiers
// 00, 03, 05), and each decoder preserves the slots it does not interpret
// in `unmapped` rather than silently dropping them.
const REQUEST_ORDINAL_OFF: usize = 0x00;
const STREAM_REF_OFF: usize = 0x08;
const DATA_OFFSET_OFF: usize = 0x18;
const SIZE_OFF: usize = 0x1c;
const WIDTH_OFF: usize = 0x30;
const HEIGHT_OFF: usize = 0x34;
const DEPTH_OFF: usize = 0x36;
const PIXEL_FORMAT_OFF: usize = 0x38;
const BYTES_PER_ROW_OFF: usize = 0x40;
const BYTES_PER_IMAGE_OFF: usize = 0x44;

// The record self-describes the request it answers (docs/findings, "Record
// fields 0x48/0x4c self-describe the requested plane/slice/level", MEASURED
// across plane 0/1 x slice 0/1 x level 0/2/3): 0x48's high 16 bits are the
// slice, and 0x4c packs plane (bits 8..16) and level (bits 0..8).
const SLICE_OFF: usize = 0x48;
const PLANE_LEVEL_OFF: usize = 0x4c;

// Pipeline records key their identity differently (dossier 03): 0x00 is a
// sequential pipeline id, not the request ordinal, and 0x08 is a 64-bit
// pipeline handle, not the streamRef.
const PIPELINE_ID_OFF: usize = 0x00;
const PIPELINE_HANDLE_OFF: usize = 0x08;

// Wireframe records are dispatch-keyed (dossier 00): streamRef (0x08) is
// -1, and 0x10 echoes the dispatchUID this record answers instead.
const WIREFRAME_DISPATCH_UID_OFF: usize = 0x10;

/// Offsets whose meaning is not established. Preserved verbatim (via
/// `unmapped`) so a later capture that populates one is visible rather than
/// silently dropped.
const TEXTURE_UNMAPPED: &[usize] = &[0x04, 0x0c, 0x10, 0x14, 0x20, 0x24, 0x28, 0x2c, 0x3c];

/// Every field this class does not interpret: the texture-mapped fields plus
/// [`REQUEST_ORDINAL_OFF`]. Buffer/heap/accel identity is `stream_ref`
/// (0x08), `data_offset` (0x18) and `size` (0x1c) alone; dossier 05 shows a
/// request ordinal in the 0x00 slot, but it is not part of this class's
/// mapped identity, so it is preserved as unmapped rather than surfaced.
const IDENTITY_UNMAPPED: &[usize] = &[
    0x00, 0x04, 0x0c, 0x10, 0x14, 0x20, 0x24, 0x28, 0x2c, 0x30, 0x34, 0x38, 0x3c, 0x40, 0x44, 0x48,
    0x4c,
];

/// Every field a pipeline record does not interpret: everything but
/// `pipeline_id` (0x00), `handle` (0x08..0x10), `data_offset` (0x18) and
/// `size` (0x1c).
const PIPELINE_UNMAPPED: &[usize] = &[
    0x04, 0x10, 0x14, 0x20, 0x24, 0x28, 0x2c, 0x30, 0x34, 0x38, 0x3c, 0x40, 0x44, 0x48, 0x4c,
];

/// Every field a wireframe record does not interpret: everything but
/// `dispatch_uid` (0x10), `data_offset` (0x18), `size` (0x1c), `width`
/// (0x30), `height` (0x34) and `pixel_format` (0x38).
const WIREFRAME_UNMAPPED: &[usize] = &[
    0x00, 0x04, 0x08, 0x0c, 0x14, 0x20, 0x24, 0x28, 0x2c, 0x3c, 0x40, 0x44, 0x48, 0x4c,
];

/// Name an info-record byte offset for use as an `unmapped` map key.
fn offset_name(o: usize) -> &'static str {
    match o {
        0x00 => "0x00",
        0x04 => "0x04",
        0x08 => "0x08",
        0x0c => "0x0c",
        0x10 => "0x10",
        0x14 => "0x14",
        0x18 => "0x18",
        0x1c => "0x1c",
        0x20 => "0x20",
        0x24 => "0x24",
        0x28 => "0x28",
        0x2c => "0x2c",
        0x30 => "0x30",
        0x34 => "0x34",
        0x38 => "0x38",
        0x3c => "0x3c",
        0x40 => "0x40",
        0x44 => "0x44",
        0x48 => "0x48",
        _ => "0x4c",
    }
}

fn unmapped_of(c: &[u8], offsets: &[usize]) -> BTreeMap<&'static str, u32> {
    let mut unmapped = BTreeMap::new();
    for &o in offsets {
        unmapped.insert(offset_name(o), u32_at(c, o));
    }
    unmapped
}

/// A texture fetch record.
#[derive(Debug, Clone)]
pub struct TextureRecord {
    /// The resource's streamRef (info field 0x08).
    pub stream_ref: u32,
    /// Per-session request ordinal (info field 0x00). Not a streamRef.
    pub request_ordinal: u32,
    /// Payload offset into the reply `data`.
    pub data_offset: u32,
    /// Payload size in bytes.
    pub size: u32,
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u16,
    /// Depth / slices.
    pub depth: u16,
    /// `MTLPixelFormat`.
    pub pixel_format: u32,
    /// Bytes per row.
    pub bytes_per_row: u32,
    /// Bytes per image.
    pub bytes_per_image: u32,
    /// The array slice (or cube face) this record answers (info field
    /// 0x48, high 16 bits). Self-describing: the value the request asked
    /// for, not derived from matching against the request.
    pub slice: u32,
    /// The texture plane this record answers (info field 0x4c, bits
    /// 8..16). Self-describing, see `slice`.
    pub plane: u32,
    /// The mip level this record answers (info field 0x4c, bits 0..8).
    /// Self-describing, see `slice`.
    pub level: u32,
    /// Preserved unmapped fields, keyed by hex offset.
    pub unmapped: BTreeMap<&'static str, u32>,
}

impl Record for TextureRecord {
    fn decode(c: &[u8]) -> Self {
        let plane_level = u32_at(c, PLANE_LEVEL_OFF);
        Self {
            request_ordinal: u32_at(c, REQUEST_ORDINAL_OFF),
            stream_ref: u32_at(c, STREAM_REF_OFF),
            data_offset: u32_at(c, DATA_OFFSET_OFF),
            size: u32_at(c, SIZE_OFF),
            width: u32_at(c, WIDTH_OFF),
            height: u16_at(c, HEIGHT_OFF),
            depth: u16_at(c, DEPTH_OFF),
            pixel_format: u32_at(c, PIXEL_FORMAT_OFF),
            bytes_per_row: u32_at(c, BYTES_PER_ROW_OFF),
            bytes_per_image: u32_at(c, BYTES_PER_IMAGE_OFF),
            slice: u32_at(c, SLICE_OFF) >> 16,
            plane: (plane_level >> 8) & 0xff,
            level: plane_level & 0xff,
            unmapped: unmapped_of(c, TEXTURE_UNMAPPED),
        }
    }
    fn data_range(&self) -> (usize, usize) {
        (self.data_offset as usize, self.size as usize)
    }
}

/// A buffer fetch record: raw bytes identified by `stream_ref` alone.
#[derive(Debug, Clone)]
pub struct BufferRecord {
    /// The resource's streamRef (info field 0x08).
    pub stream_ref: u32,
    /// Payload offset into the reply `data`.
    pub data_offset: u32,
    /// Payload size in bytes.
    pub size: u32,
    /// Preserved unmapped fields, keyed by hex offset.
    pub unmapped: BTreeMap<&'static str, u32>,
}

impl Record for BufferRecord {
    fn decode(c: &[u8]) -> Self {
        Self {
            stream_ref: u32_at(c, STREAM_REF_OFF),
            data_offset: u32_at(c, DATA_OFFSET_OFF),
            size: u32_at(c, SIZE_OFF),
            unmapped: unmapped_of(c, IDENTITY_UNMAPPED),
        }
    }
    fn data_range(&self) -> (usize, usize) {
        (self.data_offset as usize, self.size as usize)
    }
}

/// A heap fetch record: raw bytes identified by `stream_ref` alone.
#[derive(Debug, Clone)]
pub struct HeapRecord {
    /// The resource's streamRef (info field 0x08).
    pub stream_ref: u32,
    /// Payload offset into the reply `data`.
    pub data_offset: u32,
    /// Payload size in bytes.
    pub size: u32,
    /// Preserved unmapped fields, keyed by hex offset.
    pub unmapped: BTreeMap<&'static str, u32>,
}

impl Record for HeapRecord {
    fn decode(c: &[u8]) -> Self {
        Self {
            stream_ref: u32_at(c, STREAM_REF_OFF),
            data_offset: u32_at(c, DATA_OFFSET_OFF),
            size: u32_at(c, SIZE_OFF),
            unmapped: unmapped_of(c, IDENTITY_UNMAPPED),
        }
    }
    fn data_range(&self) -> (usize, usize) {
        (self.data_offset as usize, self.size as usize)
    }
}

/// An acceleration-structure fetch record: raw bytes identified by
/// `stream_ref` alone.
#[derive(Debug, Clone)]
pub struct AccelRecord {
    /// The resource's streamRef (info field 0x08).
    pub stream_ref: u32,
    /// Payload offset into the reply `data`.
    pub data_offset: u32,
    /// Payload size in bytes.
    pub size: u32,
    /// Preserved unmapped fields, keyed by hex offset.
    pub unmapped: BTreeMap<&'static str, u32>,
}

impl Record for AccelRecord {
    fn decode(c: &[u8]) -> Self {
        Self {
            stream_ref: u32_at(c, STREAM_REF_OFF),
            data_offset: u32_at(c, DATA_OFFSET_OFF),
            size: u32_at(c, SIZE_OFF),
            unmapped: unmapped_of(c, IDENTITY_UNMAPPED),
        }
    }
    fn data_range(&self) -> (usize, usize) {
        (self.data_offset as usize, self.size as usize)
    }
}

/// A pipeline-binaries fetch record (command-stream-keyed, nested payload).
#[derive(Debug, Clone)]
pub struct PipelineRecord {
    /// Sequential pipeline id (info field 0x00).
    pub pipeline_id: u32,
    /// 64-bit pipeline handle (info field 0x08).
    pub handle: u64,
    /// Payload offset into `data` (the nested bplist of Mach-O + stats).
    pub data_offset: u32,
    /// Payload size.
    pub size: u32,
    /// Preserved unmapped fields.
    pub unmapped: BTreeMap<&'static str, u32>,
}

impl Record for PipelineRecord {
    fn decode(c: &[u8]) -> Self {
        Self {
            pipeline_id: u32_at(c, PIPELINE_ID_OFF),
            handle: u64_at(c, PIPELINE_HANDLE_OFF),
            data_offset: u32_at(c, DATA_OFFSET_OFF),
            size: u32_at(c, SIZE_OFF),
            unmapped: unmapped_of(c, PIPELINE_UNMAPPED),
        }
    }
    fn data_range(&self) -> (usize, usize) {
        (self.data_offset as usize, self.size as usize)
    }
}

/// A wireframe fetch record (dispatch-keyed; a rendered image).
#[derive(Debug, Clone)]
pub struct WireframeRecord {
    /// The dispatchUID this record answers (info field 0x10). streamRef
    /// (0x08) is -1.
    pub dispatch_uid: u32,
    /// Rendered image width.
    pub width: u32,
    /// Rendered image height.
    pub height: u16,
    /// `MTLPixelFormat` of the rendered image.
    pub pixel_format: u32,
    /// Payload offset into `data`.
    pub data_offset: u32,
    /// Payload size.
    pub size: u32,
    /// Preserved unmapped fields.
    pub unmapped: BTreeMap<&'static str, u32>,
}

impl Record for WireframeRecord {
    fn decode(c: &[u8]) -> Self {
        Self {
            dispatch_uid: u32_at(c, WIREFRAME_DISPATCH_UID_OFF),
            width: u32_at(c, WIDTH_OFF),
            height: u16_at(c, HEIGHT_OFF),
            pixel_format: u32_at(c, PIXEL_FORMAT_OFF),
            data_offset: u32_at(c, DATA_OFFSET_OFF),
            size: u32_at(c, SIZE_OFF),
            unmapped: unmapped_of(c, WIREFRAME_UNMAPPED),
        }
    }
    fn data_range(&self) -> (usize, usize) {
        (self.data_offset as usize, self.size as usize)
    }
}

/// A decoded reply: owns the raw bytes, yields typed records borrowing
/// `data`.
pub struct Reply<T: Record> {
    raw: RawReply,
    records: Vec<T>,
}

impl<T: Record> Reply<T> {
    /// Decode a reply's info table into typed records.
    pub fn decode(raw: RawReply) -> Result<Self, FetchError> {
        if !raw.info.len().is_multiple_of(T::STRIDE) {
            return Err(FetchError::Parse(format!(
                "info table {} not a multiple of stride {}",
                raw.info.len(),
                T::STRIDE
            )));
        }
        // `as_chunks::<T::STRIDE>()` (clippy's suggestion) does not compile:
        // a generic type parameter's associated const cannot be used as a
        // const-generic argument on stable Rust ("generic parameters may
        // not be used in const operations"). `T::STRIDE` is only known at
        // each monomorphization, not at the point `as_chunks` needs a fixed
        // array length, so `chunks_exact` stays the correct tool here.
        #[allow(clippy::chunks_exact_to_as_chunks)]
        let records: Vec<T> = raw.info.chunks_exact(T::STRIDE).map(T::decode).collect();
        // Every record's payload must lie within the `data` blob. Surfacing an
        // out-of-range `(offset, size)` here (rather than letting `payload`
        // silently return `&[]`) turns a decode/offset mismatch into a visible
        // error instead of an empty payload indistinguishable from a genuinely
        // empty one - the fidelity this substrate layer promises.
        for r in &records {
            let (o, n) = r.data_range();
            o.checked_add(n)
                .filter(|end| *end <= raw.data.len())
                .ok_or_else(|| {
                    FetchError::Parse(format!(
                        "record payload [{o}, {o}+{n}) exceeds the {}-byte data blob",
                        raw.data.len()
                    ))
                })?;
        }
        Ok(Self { raw, records })
    }
    /// The decoded records.
    pub fn records(&self) -> &[T] {
        &self.records
    }
    /// The payload bytes for one record (borrowed from the owned `data`).
    ///
    /// Always in-bounds: [`Reply::decode`] validated every record's
    /// `(offset, size)` against the `data` blob, so a slice always exists.
    pub fn payload(&self, r: &T) -> &[u8] {
        let (o, n) = r.data_range();
        self.raw.data.get(o..o + n).unwrap_or(&[])
    }
    /// The raw reply envelope.
    pub fn raw(&self) -> &RawReply {
        &self.raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/reply_corpus_64x64.plist"
        ))
        .expect("fixture missing")
    }
    #[test]
    fn parses_the_envelope() {
        let r = RawReply::parse(&fixture()).unwrap();
        assert_eq!(r.unknown_len, 0);
        assert_eq!(r.info.len() % 80, 0);
        assert!(!r.data.is_empty());
    }

    #[test]
    fn the_corpus_reply_has_182_records_with_the_known_format_mix() {
        let raw = RawReply::parse(&fixture()).unwrap();
        let records: Vec<TextureRecord> = raw
            .info
            .as_chunks::<{ TextureRecord::STRIDE }>()
            .0
            .iter()
            .map(|chunk| TextureRecord::decode(chunk))
            .collect();
        assert_eq!(records.len(), 182);
        let count = |fmt: u32| records.iter().filter(|r| r.pixel_format == fmt).count();
        assert_eq!(count(10), 8, "R8Unorm");
        assert_eq!(count(70), 2, "RGBA8Unorm");
        assert_eq!(count(80), 162, "BGRA8Unorm");
        assert_eq!(count(125), 10, "RGBA32Float");
    }

    /// Build an 80-byte chunk with `u32`s planted at the given offsets.
    fn chunk(fields: &[(usize, u32)]) -> [u8; 80] {
        let mut c = [0u8; 80];
        for &(o, v) in fields {
            c[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        c
    }

    #[test]
    fn decodes_a_texture_record() {
        let c = chunk(&[
            (REQUEST_ORDINAL_OFF, 25),
            (STREAM_REF_OFF, 24),
            (DATA_OFFSET_OFF, 4096),
            (SIZE_OFF, 16384),
            (WIDTH_OFF, 64),
            (HEIGHT_OFF, 64 | (1 << 16)), // height 64, depth 1
            (PIXEL_FORMAT_OFF, 80),
            (BYTES_PER_ROW_OFF, 256),
            (BYTES_PER_IMAGE_OFF, 16384),
        ]);
        let r = TextureRecord::decode(&c);
        assert_eq!(r.request_ordinal, 25);
        assert_eq!(r.stream_ref, 24);
        assert_eq!(r.data_offset, 4096);
        assert_eq!(r.size, 16384);
        assert_eq!(r.width, 64);
        assert_eq!(r.height, 64);
        assert_eq!(r.depth, 1);
        assert_eq!(r.pixel_format, 80);
        assert_eq!(r.bytes_per_row, 256);
        assert_eq!(r.bytes_per_image, 16384);
        assert_eq!(r.slice, 0);
        assert_eq!(r.plane, 0);
        assert_eq!(r.level, 0);
        assert_eq!(r.data_range(), (4096, 16384));
    }

    #[test]
    fn decodes_the_self_describing_plane_slice_level() {
        // MEASURED (docs/findings/00-texture-fetch.md): plane1/slice1/level2
        // -> 0x48 == 0x10000, 0x4c == 0x102.
        let c = chunk(&[(SLICE_OFF, 1 << 16), (PLANE_LEVEL_OFF, (1 << 8) | 2)]);
        let r = TextureRecord::decode(&c);
        assert_eq!(r.slice, 1);
        assert_eq!(r.plane, 1);
        assert_eq!(r.level, 2);
    }

    #[test]
    fn decodes_identity_layout_records() {
        let c = chunk(&[
            (STREAM_REF_OFF, 42),
            (DATA_OFFSET_OFF, 1000),
            (SIZE_OFF, 2000),
        ]);

        let b = BufferRecord::decode(&c);
        assert_eq!(b.stream_ref, 42);
        assert_eq!(b.data_range(), (1000, 2000));

        let h = HeapRecord::decode(&c);
        assert_eq!(h.stream_ref, 42);
        assert_eq!(h.data_range(), (1000, 2000));

        let a = AccelRecord::decode(&c);
        assert_eq!(a.stream_ref, 42);
        assert_eq!(a.data_range(), (1000, 2000));
    }

    #[test]
    fn decodes_a_pipeline_record() {
        let mut c = [0u8; 80];
        c[PIPELINE_ID_OFF..PIPELINE_ID_OFF + 4].copy_from_slice(&1184u32.to_le_bytes());
        c[PIPELINE_HANDLE_OFF..PIPELINE_HANDLE_OFF + 8]
            .copy_from_slice(&0x78f59d3000u64.to_le_bytes());
        c[DATA_OFFSET_OFF..DATA_OFFSET_OFF + 4].copy_from_slice(&36758u32.to_le_bytes());
        c[SIZE_OFF..SIZE_OFF + 4].copy_from_slice(&567625u32.to_le_bytes());

        let r = PipelineRecord::decode(&c);
        assert_eq!(r.pipeline_id, 1184);
        assert_eq!(r.handle, 0x78f59d3000);
        assert_eq!(r.data_offset, 36758);
        assert_eq!(r.size, 567625);
        assert_eq!(r.data_range(), (36758, 567625));
    }

    #[test]
    fn decodes_a_wireframe_record() {
        let c = chunk(&[
            (STREAM_REF_OFF, u32::MAX), // -1: not part of this class's identity
            (WIREFRAME_DISPATCH_UID_OFF, 325),
            (DATA_OFFSET_OFF, 8192),
            (SIZE_OFF, 320 * 288),
            (WIDTH_OFF, 320),
            (HEIGHT_OFF, 288),
            (PIXEL_FORMAT_OFF, 10), // R8Unorm
        ]);
        let r = WireframeRecord::decode(&c);
        assert_eq!(r.dispatch_uid, 325);
        assert_eq!(r.width, 320);
        assert_eq!(r.height, 288);
        assert_eq!(r.pixel_format, 10);
        assert_eq!(r.data_range(), (8192, 320 * 288));
        assert_eq!(r.unmapped[offset_name(STREAM_REF_OFF)], u32::MAX);
    }

    #[test]
    fn decodes_the_fixture_into_a_reply() {
        let raw = RawReply::parse(&fixture()).unwrap();
        let reply = Reply::<TextureRecord>::decode(raw).unwrap();
        assert_eq!(reply.records().len(), 182);
        let first = &reply.records()[0];
        assert_eq!(reply.payload(first).len(), first.size as usize);
    }

    #[test]
    fn decode_rejects_a_record_whose_payload_exceeds_the_data_blob() {
        // One texture record claiming a 50-byte payload at offset 100, but a
        // data blob only 10 bytes long: the payload runs off the end.
        let raw = RawReply {
            unknown_len: 0,
            info: chunk(&[(DATA_OFFSET_OFF, 100), (SIZE_OFF, 50)]).to_vec(),
            data: vec![0u8; 10],
        };
        let res = Reply::<TextureRecord>::decode(raw);
        assert!(matches!(res, Err(FetchError::Parse(_))));
    }

    #[test]
    fn decode_accepts_a_record_whose_payload_exactly_fills_the_data_blob() {
        // offset 0 + size 10 == data.len() 10: the boundary is in-bounds.
        let raw = RawReply {
            unknown_len: 0,
            info: chunk(&[(DATA_OFFSET_OFF, 0), (SIZE_OFF, 10)]).to_vec(),
            data: vec![7u8; 10],
        };
        let reply = Reply::<TextureRecord>::decode(raw).unwrap();
        assert_eq!(reply.payload(&reply.records()[0]), &[7u8; 10]);
    }
}
