# RE backlog

Prioritised, concrete, checkable reverse-engineering tasks so the campaign is
self-directing rather than prompt-driven. Work top-down; each item says its
done-criterion. Move finished items to "Done" with the commit that closed them.
Tooling: `probes/run.sh <probe>` (serialised replayer), `rawfetch <Class>`
(any fetch/decode class), `classdump <Name>` (live runtime shape), `gpudebug`
(oracle), `fixture-apps/` (+ `capture-late.sh`, `ACCEL_VERTS`).

## Now (reachable on existing captures / cheap fixtures)


5. **Graduate the renderer classes by runtime shape.** GTMTLTextureRenderer /
   GTMTLTextureRenderEncoder (the fetch's image renderer) via `classdump`;
   GTGPUAPSConfig. DONE when their shapes are established in dossier 06 with a
   note on role.

## Next (need a live probe or more setup)


## Later (bigger / lower value)

8. DONE - shader-profiler classes + the 4 remaining C functions
   (generateDerivedDataPayload/CLI/fillError/handleError) graduated by ipsw
   prologue arity.
9. DONE - GT_ENV (config global) + fillError/handleError/CLI (ipsw arity).

## Blocked / deprioritised

10. Entitled XPC path (GTMTLReplayClient_{create,destroy}NewTransport,
    GTTransportMessage_replayer) - the driving PROTOCOL is obsolete (handoff 5F),
    but the symbol SIGNATURES are now established (ipsw arity + runtime shape).

## Done

- Fetch family mapped; buffer/heap ground-truth (a076c4c).
- Acceleration-structure fetch + byte-format decode (b672f30, 774216c).
- Pipeline-binaries reply format (7aa78b6).
- Fetch/decode class graduation by runtime shape (1d7d3ce).
- Nested pipeline payload decoded (item 3); renderer/APS classes graduated (item 5).
- Accel count field (0x2c=triangle count) + size scaling (item 2).
- Instance (top-level) accel structure: fetches empty payload; 0x20 kind flag (item 1).
- Fetch taxonomy: resource-keyed vs dispatch-keyed families (item 4). "Now" section cleared.
- preferDevice audit (item 6): faults in-process, root cause [client+0x30]=null; dead end, HANDOFF 3 resolved.
- Dispatch-keyed fetches driven (item 11): wireframe by draw-index dispatchUID -> rendered image; 62/98 draws.
- Harvester getters confirmed live by synthetic-block round-trip (item 7); coverage 18/31.
- GT_ENV confirmed as the config global (item 9); coverage 19/31.
- Harvester refinements: MTLTexture-N bundle files ARE capture blocks (real block source); 0x30 plane descriptor decoded (6 u64s: fmt/w/h/depth/bpr/size).
- Shader-profiler data classes graduated by runtime shape (item 8); coverage 22/31.
- 4 C functions (generateDerivedDataPayload/CLI/fillError/handleError) graduated by ipsw prologue arity; coverage 26/31.
- Transport signatures (create/destroyNewTransport arity, GTTransportMessage_replayer shape) + GTReplayUnarchiver shape; coverage 30/31.

## Coverage complete (30/31)

The only unverified symbol is GTMTLReplayClient_preferDevice, a resolved dead
end in-process (item 6). Every establishable symbol is established. The natural
next work is thread A (build the safe crate), not more RE.
