//! Fetched pipeline-binaries records (dossier 03): raw nested bplist always
//! available, compiled stages and performance statistics parsed on demand.
//!
//! **The nested payload is an `NSKeyedArchiver` bplist, not a plain one.**
//! `plist::from_bytes` alone does NOT hand back the object graph it
//! archives: it hands back the raw envelope (`$version`/`$archiver`/`$top`/
//! `$objects`), with every object reference left as a `plist::Value::Uid`
//! and every `NSDictionary`/`NSArray` still in its on-disk shape (an
//! `NS.keys`/`NS.objects` pair, or an `NS.objects`-only array).
//! `unarchive_root` walks that graph and resolves it into a plain,
//! self-contained `plist::Dictionary` - MEASURED against the real nested
//! bplist inside `captures/known-buffers.gputrace`'s
//! `GTReplayFetchPipelineBinaries` reply (dumped via `probes/run.sh
//! rawfetch GTReplayFetchPipelineBinaries captures/known-buffers.gputrace`).

use crate::Error;
use crate::bytes::Payload;
use plist::Value;

/// One compiled pipeline stage.
#[derive(Debug, Clone)]
pub struct Stage {
    /// Which stage this is.
    pub kind: StageKind,
    /// The compiled GPU binary (a Mach-O container, magic `cf fa ed fe`).
    pub mach_o: Vec<u8>,
    /// The per-function id (`uniqueId`), also indexing `PerformanceStatistics`.
    pub unique_id: u64,
}

/// The kind of a pipeline stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageKind {
    /// A vertex stage.
    Vertex,
    /// A fragment stage.
    Fragment,
    /// A compute stage.
    Compute,
    /// A stage keyed by a name this decoder does not otherwise recognise.
    Other(String),
}

/// Parsed pipeline performance statistics (the `PerformanceStatistics` entry).
#[derive(Debug, Clone)]
pub struct Stats {
    raw: plist::Dictionary,
}

impl Stats {
    /// The raw statistics dictionary (nothing dropped): per-stage AGC-compiler
    /// metrics (instruction counts, register counts, threadgroup memory,
    /// compilation time), plus the raw LLVM optimization remarks.
    pub fn raw(&self) -> &plist::Dictionary {
        &self.raw
    }
}

/// A fetched pipeline-binaries record. Raw nested bplist always; parsed on
/// demand.
pub struct Pipeline {
    pipeline_id: u32,
    handle: u64,
    payload: Payload,
}

impl Pipeline {
    /// Builds a `Pipeline` from a fetched `PipelineRecord`'s id, handle and
    /// payload. Called by `Capture::pipeline_binaries`.
    pub(crate) fn from_parts(pipeline_id: u32, handle: u64, payload: Payload) -> Self {
        Self {
            pipeline_id,
            handle,
            payload,
        }
    }

    /// Sequential pipeline id.
    pub fn pipeline_id(&self) -> u32 {
        self.pipeline_id
    }
    /// 64-bit pipeline handle.
    pub fn handle(&self) -> u64 {
        self.handle
    }
    /// The raw nested bplist. Always available.
    pub fn raw_bytes(&self) -> &[u8] {
        self.payload.bytes()
    }

    /// Parse the nested bplist into compiled stages (Mach-O + id).
    ///
    /// **Deviation from the dossier-03 sketch, MEASURED against the real
    /// fixture:** each stage key's value is an `NSArray` of `{data,
    /// uniqueId}` dicts, not a single dict - so every array entry is decoded
    /// (in practice always exactly one, but nothing is dropped if there is
    /// more than one). The real key for a compute pipeline is `compute`
    /// (confirmed on `captures/known-buffers.gputrace`, a compute-only
    /// fixture); `vertex`/`fragment` are dossier 03's names for a render
    /// pipeline and are read the same way, unverified against a render
    /// fixture in this task. `*-dynamic-libraries` entries are intentionally
    /// skipped (empty in every fixture examined).
    pub fn stages(&self) -> Result<Vec<Stage>, Error> {
        let dict = unarchive_root(self.raw_bytes())?;
        let mut stages = Vec::new();
        for (key, kind) in [
            ("vertex", StageKind::Vertex),
            ("fragment", StageKind::Fragment),
            ("compute", StageKind::Compute),
        ] {
            let Some(arr) = dict.get(key).and_then(Value::as_array) else {
                continue;
            };
            for entry in arr {
                let s = entry
                    .as_dictionary()
                    .ok_or_else(|| Error::BadPipeline(format!("{key}[] entry not a dict")))?;
                let mach_o = s
                    .get("data")
                    .and_then(Value::as_data)
                    .ok_or_else(|| Error::BadPipeline(format!("{key}.data missing")))?
                    .to_vec();
                let unique_id = s
                    .get("uniqueId")
                    .and_then(Value::as_unsigned_integer)
                    .unwrap_or(0);
                stages.push(Stage {
                    kind: kind.clone(),
                    mach_o,
                    unique_id,
                });
            }
        }
        Ok(stages)
    }

    /// Parse the `PerformanceStatistics` entry (nothing dropped).
    ///
    /// **Deviation from the dossier-03 sketch, MEASURED against the real
    /// fixture:** `PerformanceStatistics` is itself an `NSArray`, not a
    /// dict keyed by stage name - one dict of raw metrics per stage, in
    /// stage order, with no name field inside identifying which stage it is.
    /// Only the ground-truthed single-stage (compute) shape is supported
    /// here: the array must hold exactly one dict, which becomes
    /// [`Stats::raw`]. A multi-stage (render) pipeline's `PerformanceStatistics`
    /// shape is unverified by this task, so more than one entry errors
    /// rather than silently picking (or merging, and possibly colliding key
    /// names between stages) one.
    pub fn performance_stats(&self) -> Result<Stats, Error> {
        let dict = unarchive_root(self.raw_bytes())?;
        let arr = dict
            .get("PerformanceStatistics")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::BadPipeline("no PerformanceStatistics".into()))?;
        match arr.as_slice() {
            [one] => {
                let raw = one
                    .as_dictionary()
                    .ok_or_else(|| {
                        Error::BadPipeline("PerformanceStatistics[0] not a dict".into())
                    })?
                    .clone();
                Ok(Stats { raw })
            }
            other => Err(Error::BadPipeline(format!(
                "PerformanceStatistics has {} entries; only the ground-truthed \
                 single-stage shape is supported",
                other.len()
            ))),
        }
    }
}

/// How deep an `NSKeyedArchiver` UID indirection may nest before
/// [`unarchive_value`] gives up - a defensive bound against a malformed or
/// (unexpectedly) cyclic object graph, not a real limit ever approached by a
/// pipeline-binaries payload.
const MAX_ARCHIVE_DEPTH: usize = 64;

/// Decode the top-level `NSDictionary` an `NSKeyedArchiver`-format `bplist`
/// wraps into a plain, self-contained [`plist::Dictionary`]: every
/// [`Value::Uid`] indirection resolved, every on-disk `NS.keys`/`NS.objects`
/// `NSDictionary` encoding turned into a real [`Value::Dictionary`], and
/// every `NS.objects`-only `NSArray` encoding turned into a real
/// [`Value::Array`]. See the module docs for why this is necessary (`plist`
/// does not unarchive `NSKeyedArchiver` on its own).
fn unarchive_root(bytes: &[u8]) -> Result<plist::Dictionary, Error> {
    let bad = |m: &str| Error::BadPipeline(m.to_string());
    let root: Value = plist::from_bytes(bytes).map_err(|e| Error::BadPipeline(e.to_string()))?;
    let envelope = root
        .as_dictionary()
        .ok_or_else(|| bad("root is not a dictionary"))?;
    let objects = envelope
        .get("$objects")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("no $objects array"))?;
    let top = envelope
        .get("$top")
        .and_then(Value::as_dictionary)
        .ok_or_else(|| bad("no $top dictionary"))?;
    let root_ref = top.get("root").ok_or_else(|| bad("no $top.root"))?;
    match unarchive_value(objects, root_ref, 0)? {
        Value::Dictionary(d) => Ok(d),
        _ => Err(bad("archived root is not a dictionary")),
    }
}

/// Resolve one node of the archived object graph: follow a `Uid` to its
/// `$objects` entry, and recursively decode `NSDictionary`/`NSArray`
/// encodings. Every other value (`Data`, `String`, `Integer`, `Real`,
/// `Boolean`, `Date`) carries no further indirection and is returned as-is.
fn unarchive_value(objects: &[Value], v: &Value, depth: usize) -> Result<Value, Error> {
    if depth > MAX_ARCHIVE_DEPTH {
        return Err(Error::BadPipeline(
            "archived object graph too deep (possible cycle)".into(),
        ));
    }
    match v {
        Value::Uid(uid) => {
            let idx = usize::try_from(uid.get())
                .map_err(|_| Error::BadPipeline("UID out of range".into()))?;
            let target = objects
                .get(idx)
                .ok_or_else(|| Error::BadPipeline(format!("UID {idx} out of bounds")))?;
            unarchive_value(objects, target, depth + 1)
        }
        Value::Dictionary(d) => {
            let Some(ns_objects) = d.get("NS.objects").and_then(Value::as_array) else {
                // Not an NS.keys/NS.objects-shaped archived collection (no
                // children to resolve, or a shape this decoder does not
                // model) - pass through unchanged.
                return Ok(v.clone());
            };
            match d.get("NS.keys").and_then(Value::as_array) {
                Some(ns_keys) => {
                    // An archived NSDictionary.
                    if ns_keys.len() != ns_objects.len() {
                        return Err(Error::BadPipeline(
                            "NS.keys/NS.objects length mismatch".into(),
                        ));
                    }
                    let mut out = plist::Dictionary::new();
                    for (k, val) in ns_keys.iter().zip(ns_objects.iter()) {
                        let key = unarchive_value(objects, k, depth + 1)?;
                        let key = key
                            .as_string()
                            .ok_or_else(|| {
                                Error::BadPipeline("a dictionary key is not a string".into())
                            })?
                            .to_string();
                        out.insert(key, unarchive_value(objects, val, depth + 1)?);
                    }
                    Ok(Value::Dictionary(out))
                }
                None => {
                    // An archived NSArray.
                    let mut out = Vec::with_capacity(ns_objects.len());
                    for val in ns_objects {
                        out.push(unarchive_value(objects, val, depth + 1)?);
                    }
                    Ok(Value::Array(out))
                }
            }
        }
        other => Ok(other.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytes::AlignedBuf;
    use std::sync::Arc;

    /// Build a minimal, hand-crafted `NSKeyedArchiver` bplist matching the
    /// real shape MEASURED on `known-buffers.gputrace`'s pipeline payload: a
    /// root `NSDictionary` whose `compute` key holds an `NSArray` of one
    /// `{data, uniqueId}` `NSDictionary`, and (optionally) a
    /// `PerformanceStatistics` key holding an `NSArray` of one flat metrics
    /// dict. `$class`/`$classname` entries are omitted: `unarchive_value`
    /// only inspects `NS.keys`/`NS.objects` shape, never `$class`.
    fn build_pipeline_bplist(mach_o: &[u8], unique_id: u64, perf: Option<(&str, i64)>) -> Vec<u8> {
        use plist::{Dictionary, Uid};

        let mut objects = vec![Value::String("$null".to_string())];
        let push = |v: Value, objects: &mut Vec<Value>| -> Uid {
            objects.push(v);
            Uid::new((objects.len() - 1) as u64)
        };

        // The stage dict: {"data": <mach_o>, "uniqueId": <unique_id>}.
        let data_key = push(Value::String("data".to_string()), &mut objects);
        let unique_id_key = push(Value::String("uniqueId".to_string()), &mut objects);
        let data_val = push(Value::Data(mach_o.to_vec()), &mut objects);
        let unique_id_val = push(Value::Integer(unique_id.into()), &mut objects);
        let mut stage_dict = Dictionary::new();
        stage_dict.insert(
            "NS.keys".to_string(),
            Value::Array(vec![Value::Uid(data_key), Value::Uid(unique_id_key)]),
        );
        stage_dict.insert(
            "NS.objects".to_string(),
            Value::Array(vec![Value::Uid(data_val), Value::Uid(unique_id_val)]),
        );
        let stage_dict_ref = push(Value::Dictionary(stage_dict), &mut objects);

        // The "compute" array: [<stage_dict>].
        let mut compute_arr = Dictionary::new();
        compute_arr.insert(
            "NS.objects".to_string(),
            Value::Array(vec![Value::Uid(stage_dict_ref)]),
        );
        let compute_arr_ref = push(Value::Dictionary(compute_arr), &mut objects);

        let mut root_keys = vec![Value::Uid(push(
            Value::String("compute".to_string()),
            &mut objects,
        ))];
        let mut root_vals = vec![Value::Uid(compute_arr_ref)];

        if let Some((metric_key, metric_val)) = perf {
            let mkey = push(Value::String(metric_key.to_string()), &mut objects);
            let mval = push(Value::Integer(metric_val.into()), &mut objects);
            let mut metrics_dict = Dictionary::new();
            metrics_dict.insert("NS.keys".to_string(), Value::Array(vec![Value::Uid(mkey)]));
            metrics_dict.insert(
                "NS.objects".to_string(),
                Value::Array(vec![Value::Uid(mval)]),
            );
            let metrics_dict_ref = push(Value::Dictionary(metrics_dict), &mut objects);

            let mut perf_arr = Dictionary::new();
            perf_arr.insert(
                "NS.objects".to_string(),
                Value::Array(vec![Value::Uid(metrics_dict_ref)]),
            );
            let perf_arr_ref = push(Value::Dictionary(perf_arr), &mut objects);

            root_keys.push(Value::Uid(push(
                Value::String("PerformanceStatistics".to_string()),
                &mut objects,
            )));
            root_vals.push(Value::Uid(perf_arr_ref));
        }

        let mut root_dict = Dictionary::new();
        root_dict.insert("NS.keys".to_string(), Value::Array(root_keys));
        root_dict.insert("NS.objects".to_string(), Value::Array(root_vals));
        let root_ref = push(Value::Dictionary(root_dict), &mut objects);

        let mut top = Dictionary::new();
        top.insert("root".to_string(), Value::Uid(root_ref));

        let mut envelope = Dictionary::new();
        envelope.insert("$version".to_string(), Value::Integer(100_000.into()));
        envelope.insert(
            "$archiver".to_string(),
            Value::String("NSKeyedArchiver".to_string()),
        );
        envelope.insert("$top".to_string(), Value::Dictionary(top));
        envelope.insert("$objects".to_string(), Value::Array(objects));

        let mut bytes = Vec::new();
        Value::Dictionary(envelope)
            .to_writer_binary(&mut bytes)
            .expect("serialize test fixture bplist");
        bytes
    }

    fn pipeline(bytes: &[u8]) -> Pipeline {
        let buf = Arc::new(AlignedBuf::from_bytes(bytes));
        Pipeline::from_parts(10, 0x100bb4450, Payload::new(buf, 0, bytes.len()))
    }

    #[test]
    fn stages_reads_the_compute_stage_mach_o_and_unique_id() {
        let mach_o: Vec<u8> = [0xcf, 0xfa, 0xed, 0xfe].into_iter().chain(0..64).collect();
        let bytes = build_pipeline_bplist(&mach_o, 12, None);
        let p = pipeline(&bytes);
        assert_eq!(p.pipeline_id(), 10);
        assert_eq!(p.handle(), 0x100bb4450);
        assert_eq!(p.raw_bytes(), bytes.as_slice());

        let stages = p.stages().unwrap();
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].kind, StageKind::Compute);
        assert_eq!(stages[0].unique_id, 12);
        assert_eq!(stages[0].mach_o, mach_o);
        assert!(stages[0].mach_o.starts_with(&[0xcf, 0xfa, 0xed, 0xfe]));
    }

    #[test]
    fn performance_stats_reads_the_single_stage_dict() {
        let mach_o = vec![0xcf, 0xfa, 0xed, 0xfe];
        let bytes = build_pipeline_bplist(&mach_o, 12, Some(("ALU instruction count", 9)));
        let p = pipeline(&bytes);
        let stats = p.performance_stats().unwrap();
        assert_eq!(
            stats
                .raw()
                .get("ALU instruction count")
                .and_then(Value::as_signed_integer),
            Some(9)
        );
    }

    #[test]
    fn performance_stats_errors_without_a_performance_statistics_key() {
        let mach_o = vec![0xcf, 0xfa, 0xed, 0xfe];
        let bytes = build_pipeline_bplist(&mach_o, 12, None);
        let p = pipeline(&bytes);
        assert!(matches!(p.performance_stats(), Err(Error::BadPipeline(_))));
    }

    #[test]
    fn performance_stats_errors_on_more_than_one_entry() {
        use plist::{Dictionary, Uid};

        // A payload whose PerformanceStatistics array holds two dicts: the
        // unverified multi-stage shape, which must error rather than
        // silently pick or merge one.
        let mach_o = vec![0xcf, 0xfa, 0xed, 0xfe];
        let mut bytes = build_pipeline_bplist(&mach_o, 1, Some(("Instruction count", 5)));
        // Re-derive with a second metrics dict appended, by round-tripping
        // through the archived object graph rather than hand-splicing bytes.
        let root: Value = plist::from_bytes(&bytes).unwrap();
        let mut envelope = root.as_dictionary().unwrap().clone();
        let objects = envelope
            .get("$objects")
            .and_then(Value::as_array)
            .unwrap()
            .clone();
        let top = envelope
            .get("$top")
            .and_then(Value::as_dictionary)
            .unwrap()
            .clone();
        let root_ref = top.get("root").unwrap().clone();
        let mut resolved = unarchive_value(&objects, &root_ref, 0).unwrap();
        if let Value::Dictionary(d) = &mut resolved {
            let mut second = Dictionary::new();
            second.insert("Instruction count".to_string(), Value::Integer(9.into()));
            if let Some(Value::Array(arr)) = d.get_mut("PerformanceStatistics") {
                arr.push(Value::Dictionary(second));
            }
        }
        // Re-archive minimally: wrap the already-resolved dict directly as
        // the reply's root (unarchive_root only requires $objects/$top/root
        // to ultimately resolve to a Value::Dictionary; a one-object graph
        // with no further Uids works just as well as the fully exploded one).
        let mut objects = vec![Value::String("$null".to_string())];
        objects.push(resolved);
        let mut top = Dictionary::new();
        top.insert("root".to_string(), Value::Uid(Uid::new(1)));
        envelope.insert("$top".to_string(), Value::Dictionary(top));
        envelope.insert("$objects".to_string(), Value::Array(objects));
        bytes.clear();
        Value::Dictionary(envelope)
            .to_writer_binary(&mut bytes)
            .unwrap();

        let p = pipeline(&bytes);
        assert!(matches!(p.performance_stats(), Err(Error::BadPipeline(_))));
    }
}
