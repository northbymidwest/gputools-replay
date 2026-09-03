//! Inventory of the framework's exported surface: the crate's coverage, made
//! measurable. Parsed from the macOS 27 SDK's GPUToolsReplay.tbd (2026-08-31).
//! `Established` means a signature or class binding in this crate is
//! probe-confirmed; `Unverified` symbols are deliberately not declared as
//! callable (a guessed signature is UB waiting to be called).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    CSymbol,
    ObjcClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Established,
    Unverified,
}

#[derive(Debug, Clone, Copy)]
pub struct Symbol {
    pub name: &'static str,
    pub kind: SymbolKind,
    pub status: Status,
    /// One line of what is known, with provenance where established.
    pub notes: &'static str,
}

use Status::{Established, Unverified};
use SymbolKind::{CSymbol, ObjcClass};

pub const EXPORTS: [Symbol; 31] = [
    // C symbols (18).
    Symbol {
        name: "GTHarvesterGetData",
        kind: CSymbol,
        status: Established,
        notes: "harvester data getter; BEHAVIOR CONFIRMED live (round-trip test the_harvester_getters_parse_a_synthetic_capture_block): returns block+metadataSize (the payload after the metadata). Dossier 02.",
    },
    Symbol {
        name: "GTHarvesterGetMetadata",
        kind: CSymbol,
        status: Established,
        notes: "harvester metadata getter; BEHAVIOR CONFIRMED live: validates magic+size, returns the block (rejects too-small/bad-magic). Dossier 02.",
    },
    Symbol {
        name: "GTHarvesterGetTexturePlane",
        kind: CSymbol,
        status: Established,
        notes: "harvester plane getter; BEHAVIOR CONFIRMED live: returns block+0x18+index*0x30. Dossier 02.",
    },
    Symbol {
        name: "GTHarvesterGetTexturePlaneCount",
        kind: CSymbol,
        status: Established,
        notes: "harvester plane-count getter; BEHAVIOR CONFIRMED live: returns [block+0x10] for a texture block. Dossier 02.",
    },
    Symbol {
        name: "GTMTLReplayClient_createNewTransport",
        kind: CSymbol,
        status: Established,
        notes: "signature established (ipsw @ 0x24f8d5144): 1 arg (mov x23,x0, the client). Opens the NSXPC transport to the entitled service (obsolete path, dossier 06); signature is a fact regardless.",
    },
    Symbol {
        name: "GTMTLReplayClient_destroyNewTransport",
        kind: CSymbol,
        status: Established,
        notes: "signature established (ipsw @ 0x24f8d5518): 0 args - releases and nulls the singleton _connection global (0x276aae000+0xf40); reads no incoming argument. Dossier 06.",
    },
    Symbol {
        name: "GTMTLReplayClient_init",
        kind: CSymbol,
        status: Established,
        notes: "two args (out-buffer, pool); prologue never reads x2-x4; see ffi",
    },
    Symbol {
        name: "GTMTLReplayClient_preferDevice",
        kind: CSymbol,
        status: Unverified,
        notes: "signature established (1 arg, the client, void); LIVE AUDIT 2026-09-01 = DEAD END in-process: faults because [client+0x30] is null on the initWithContext: client (fed to getObject); needs the transport-path client. OUT-OF-PROCESS also blocked: createNewTransport crashes in our unentitled process (SIGSEGV in its own apr_pool_create_ex before reaching the XPC connection); the path works only via the entitled gpudebug. Behavior already established statically, so this is a resolved dead end. Dossier 06.",
    },
    Symbol {
        name: "GTMTLReplayController_init",
        kind: CSymbol,
        status: Established,
        notes: "one arg (pool); read off GPUToolsReplayService; see ffi",
    },
    Symbol {
        name: "GTMTLReplayController_playAll",
        kind: CSymbol,
        status: Established,
        notes: "1 arg (controller); signature from framework disassembly, dossier 01; see ffi",
    },
    Symbol {
        name: "GTMTLReplayController_playTo",
        kind: CSymbol,
        status: Established,
        notes: "2 args (controller, uint32 target index); signature from framework disassembly, dossier 01; best hypothesis for coverage gaps; see ffi",
    },
    Symbol {
        name: "GTMTLReplayController_rewind",
        kind: CSymbol,
        status: Established,
        notes: "1 arg (controller); signature from framework disassembly, dossier 01; see ffi",
    },
    Symbol {
        name: "GTMTLReplayErrorHandling_initWithObserver",
        kind: CSymbol,
        status: Established,
        notes: "one arg, object answering -notifyError:; see ffi",
    },
    Symbol {
        name: "GTMTLReplayHost_generateDerivedDataPayload",
        kind: CSymbol,
        status: Established,
        notes: "signature established from the prologue (ipsw disass @ 0x24f88e270): 2 args (mov x21,x1 / mov x23,x0; no x2+ read). Generates profiler derived-data payload. Dossier 04.",
    },
    Symbol {
        name: "GTMTLReplay_CLI",
        kind: CSymbol,
        status: Established,
        notes: "signature established from the prologue (@ 0x24f8c5ba4): 3 args (x0 stored, x1->x22, x2->x20). A CLI entry point. Dossier 06.",
    },
    Symbol {
        name: "GTMTLReplay_fillError",
        kind: CSymbol,
        status: Established,
        notes: "signature established from the prologue (@ 0x24f8918d8): 3 args (out NSError** x0, then x1/x2 to _MakeNSError; result stored at *x0). Dossier 06.",
    },
    Symbol {
        name: "GTMTLReplay_handleError",
        kind: CSymbol,
        status: Established,
        notes: "signature established from the prologue (@ 0x24f890bd4): 6 args (x0..x5 all saved before use). Error handler. Dossier 06.",
    },
    Symbol {
        name: "GT_ENV",
        kind: CSymbol,
        status: Established,
        notes: "the framework global config/env table (DATA symbol); BEHAVIOR CONFIRMED live (probes/run.sh gtenv): config word at GT_ENV+0x30 is 0 before GTMTLReplayController_init and populated after, with bit 11 set from MTLREPLAYER_FORCE_RESOURCES_RESIDENT=1. Dossier 06.",
    },
    // ObjC classes (13).
    Symbol {
        name: "GTGPUAPSConfig",
        kind: ObjcClass,
        status: Established,
        notes: "GPU performance/tracing config; shape fully described by the live runtime (2026-09-01), instanceSize 200: duration, pulsePeriod, tileTracing, countPeriod, eslInstTracing, cliqueTraceLevel, toDictionary. Role from selectors (profiler surface, dossier 04); not behavior-probed. Dossier 06.",
    },
    Symbol {
        name: "GTMTLReplayService",
        kind: ObjcClass,
        status: Established,
        notes: "-initWithContext:, -load:error: (NSURL), -fetch:; HANDOFF 2.2/2.4",
    },
    Symbol {
        name: "GTMTLTextureRenderEncoder",
        kind: ObjcClass,
        status: Established,
        notes: "command encoder for GTMTLTextureRenderer; shape fully described by the live runtime (2026-09-01), instanceSize 32: drawTexture:/drawOverlay:/setTransform:/setBounds:/submitCommand. Dossier 06.",
    },
    Symbol {
        name: "GTMTLTextureRenderer",
        kind: ObjcClass,
        status: Established,
        notes: "texture-to-view preview renderer; shape fully described by the live runtime (2026-09-01), instanceSize 64: initWithDevice:, render:withEncoder:withFormat:renderTargetSize:viewContentsScale:, renderTexture:/renderOverlay: with CATransform3D/CGRect. Role from selectors; not behavior-probed. Dossier 06.",
    },
    Symbol {
        name: "GTMutableShaderProfilerStreamData",
        kind: ObjcClass,
        status: Established,
        notes: "shader-profiler trace stream (BUILDER side); shape fully described by the live runtime, instanceSize 352: addString:/addCommandBuffers:count:/addEncoders:count:/addGPUCommands:count:/addPipelineStates:count:/addShaderFunctionInfo:count:/addAPSData:/addPipelinePerformanceStatisticsData:. Role from selectors. Dossier 04.",
    },
    Symbol {
        name: "GTReplayDecodeGenericAccelerationStructure",
        kind: ObjcClass,
        status: Established,
        notes: "accel-structure decode request; live runtime: instanceSize 32, setStreamRef:(Q)+setDispatchUID: fetch-request shape identical to GTReplayFetchTexture's base. Pinned by regression test. Live fetch not yet run.",
    },
    Symbol {
        name: "GTReplayFetchAccelerationStructure",
        kind: ObjcClass,
        status: Established,
        notes: "accel-structure fetch, new in 27 SDK, BEHAVIOR CONFIRMED live (dossier 05) on a purpose-built raytracing capture: per-resource streamRef-keyed (texture info layout, 0x08=streamRef), reply data is RAW accel-structure bytes (not a nested archive). Shape pinned by regression test.",
    },
    Symbol {
        name: "GTReplayFetchPipelineBinaries",
        kind: ObjcClass,
        status: Established,
        notes: "pipeline-binaries fetch, BEHAVIOR CONFIRMED live on corpus (dossier 03): same bplist unknown/info/data reply, 80-byte info records, requested streamRef is a command-stream threshold (cumulative reply), each payload a nested bplist of compiled vertex/fragment Mach-O binaries + PerformanceStatistics. Shape pinned by regression test.",
    },
    Symbol {
        name: "GTReplayFetchTexture",
        kind: ObjcClass,
        status: Established,
        notes: "fully described by the runtime, instanceSize 128; HANDOFF 2.4",
    },
    Symbol {
        name: "GTReplayUnarchiver",
        kind: ObjcClass,
        status: Established,
        notes: "shape fully described by the live runtime: instanceSize 8, NO own methods (a thin NSObject subclass/wrapper). The runtime description is complete; its role beyond the empty shell is not established (no distinguishing selectors to probe). Dossier 00.",
    },
    Symbol {
        name: "GTShaderProfilerBinaryAnalysisResult",
        kind: ObjcClass,
        status: Established,
        notes: "compiled-shader instruction analysis; shape fully described by the live runtime, instanceSize 328: instructions ({IIQQIICCS} per instr)/instructionCount/clauses ({QQIIII})/binaryRanges/binaryLocations ({IIII})/stringAtIndex:. Role from selectors. Dossier 04.",
    },
    Symbol {
        name: "GTShaderProfilerStreamData",
        kind: ObjcClass,
        status: Established,
        notes: "shader-profiler trace stream (READ side); shape fully described by the live runtime (2026-09-01), instanceSize 344: deviceInfo/version/strings/traceName/pipelineStates/encoders/GPUCommandInfoFromFunctionIndex:subCommandIndex:/encode:error:, NSCoding. Selectors reveal record types (encoders {QQQIIII}, GPU commands {IIIIQIi}). Role from selectors. Dossier 04.",
    },
    Symbol {
        name: "GTTransportMessage_replayer",
        kind: ObjcClass,
        status: Established,
        notes: "XPC transport message; shape fully described by the live runtime, instanceSize 72: transport/payload/kind/serial/attributes/attributeForKey:/boolForKey:/doubleForKey:. Dossier 06.",
    },
];

/// Classes the established path uses that the tbd does NOT export; they are
/// registered with the runtime when the framework loads.
pub const RUNTIME_ONLY_CLASSES: [&str; 1] = ["GTReplayRequestBatch"];

/// (established, total) over the exported surface.
pub fn coverage() -> (usize, usize) {
    let established = EXPORTS
        .iter()
        .filter(|s| s.status == Status::Established)
        .count();
    (established, EXPORTS.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::runtime::AnyClass;
    use std::ffi::CString;

    #[test]
    fn the_inventory_matches_the_tbd_counts() {
        let classes = EXPORTS
            .iter()
            .filter(|s| s.kind == SymbolKind::ObjcClass)
            .count();
        let c_symbols = EXPORTS
            .iter()
            .filter(|s| s.kind == SymbolKind::CSymbol)
            .count();
        assert_eq!(classes, 13);
        assert_eq!(c_symbols, 18);
        assert_eq!(EXPORTS.len(), 31);
    }

    #[test]
    fn no_duplicate_names() {
        let mut names: Vec<&str> = EXPORTS.iter().map(|s| s.name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate symbol name in EXPORTS");
    }

    #[test]
    fn coverage_counts_established_over_total() {
        let (established, total) = coverage();
        assert_eq!(total, 31);
        // Bootstrap era (3 C functions + 2 classes), the playback surface
        // (playAll/playTo/rewind, dossier 01), and the fetch/decode request
        // classes graduated by live-runtime shape (pipeline-binaries,
        // accel-structure fetch, accel-structure decode; dossiers 03/05), and
        // the two texture-preview renderer classes + GTGPUAPSConfig graduated
        // by live-runtime shape (dossier 06), and the four harvester getters
        // confirmed by a synthetic-block round-trip (dossier 02), GT_ENV
        // confirmed as the config global (dossier 06), and the three
        // shader-profiler data classes graduated by live-runtime shape
        // (dossier 04), and four C functions (generateDerivedDataPayload, CLI,
        // fillError, handleError) with arities read off the prologue via ipsw
        // (dossiers 04/06).
        // Update this alongside the EXPORTS table as the campaign graduates
        // surfaces.
        assert_eq!(established, 30);
    }

    /// Every class the inventory lists must be registered with the live
    /// runtime; the framework is linked, so they load before main.
    #[test]
    fn every_inventoried_class_is_registered_with_the_runtime() {
        for symbol in EXPORTS.iter().filter(|s| s.kind == SymbolKind::ObjcClass) {
            let name = CString::new(symbol.name).unwrap();
            assert!(
                AnyClass::get(&name).is_some(),
                "{} is inventoried as a class but is not registered",
                symbol.name
            );
        }
        for name in RUNTIME_ONLY_CLASSES {
            let cname = CString::new(name).unwrap();
            assert!(AnyClass::get(&cname).is_some(), "{name} is not registered");
        }
    }

    /// The fetch/decode request classes graduated by shape (dossiers 03/05)
    /// really carry the `-fetch:`-request contract this crate depends on:
    /// instanceSize 32 and a `setStreamRef:`/`setDispatchUID:` pair whose
    /// runtime type encodings match `GTReplayFetchTexture`'s base. If a future
    /// OS changes any of this, that is a fact worth failing the build over,
    /// exactly like the client-encoding gate. MEASURED against the live runtime,
    /// not the classdump text.
    #[test]
    fn the_fetch_request_classes_share_the_streamref_shape() {
        use std::ffi::CStr;
        // (selector, expected type encoding) read live off GTReplayFetchTexture.
        const SET_STREAM_REF: &str = "v24@0:8Q16";
        const SET_DISPATCH_UID: &str = "v24@0:8(?={?=ii}Q)16";

        fn encoding_of(cls: &AnyClass, selector: &str) -> Option<String> {
            cls.instance_methods().iter().find_map(|m| {
                if m.name().name().to_string_lossy() == selector {
                    let ptr = unsafe { objc2::ffi::method_getTypeEncoding(*m) };
                    (!ptr.is_null()).then(|| {
                        unsafe { CStr::from_ptr(ptr) }
                            .to_string_lossy()
                            .into_owned()
                    })
                } else {
                    None
                }
            })
        }

        // The reference: the established texture class must still show the base
        // shape, or the constants above are stale.
        let tex = AnyClass::get(&CString::new("GTReplayFetchTexture").unwrap())
            .expect("GTReplayFetchTexture not registered");
        assert_eq!(
            encoding_of(tex, "setStreamRef:").as_deref(),
            Some(SET_STREAM_REF)
        );
        assert_eq!(
            encoding_of(tex, "setDispatchUID:").as_deref(),
            Some(SET_DISPATCH_UID)
        );

        for name in [
            "GTReplayFetchPipelineBinaries",
            "GTReplayFetchAccelerationStructure",
            "GTReplayDecodeGenericAccelerationStructure",
        ] {
            let cls = AnyClass::get(&CString::new(name).unwrap())
                .unwrap_or_else(|| panic!("{name} not registered"));
            assert_eq!(cls.instance_size(), 32, "{name} instanceSize changed");
            assert_eq!(
                encoding_of(cls, "setStreamRef:").as_deref(),
                Some(SET_STREAM_REF),
                "{name} setStreamRef: encoding changed"
            );
            assert_eq!(
                encoding_of(cls, "setDispatchUID:").as_deref(),
                Some(SET_DISPATCH_UID),
                "{name} setDispatchUID: encoding changed"
            );
        }
    }

    #[test]
    fn accel_structure_is_noted_as_new_in_27() {
        let s = EXPORTS
            .iter()
            .find(|s| s.name == "GTReplayFetchAccelerationStructure")
            .expect("accel-structure fetch missing from inventory");
        assert!(
            s.notes.contains("27"),
            "note should record its 27-only status"
        );
    }
}
