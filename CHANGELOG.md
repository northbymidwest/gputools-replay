# Changelog

Notable changes per release. Dates are the publish date.

## 0.2.0 - 2026-09-09

Session-based texture descriptors: descriptors are now read off the live
`MTLTexture` the replayer created, keyed by streamRef, instead of parsing the
`.gputrace` manifest and joining by creation-order ordinal. This is correct
across capture serialization schemas where the offline heuristic broke (a
consumer reported 0 descriptors on an SDL3 capture).

### Added

- `Session::texture_descriptor(stream_ref)` (`gputools-replay`) and
  `Capture::texture_descriptor(stream_ref)` (`gputools-replay-hl`): the
  authoritative descriptor for a loaded texture, read from the replayer's live
  object map. Returns `Result<Option<TextureDescriptor>, ObjectMapError>` -
  `Ok(None)` for a streamRef that is not a loaded texture, `Err` only if the
  object map itself cannot be reached. `ObjectMapError` is re-exported from both
  crates.
- `gputools-replay-sys`: a `GTMTLReplayObjectMap` binding and the
  `controller_object_map` accessor (in a new `layout` module, which also now
  holds `ClientBuffer`/`CLIENT_BUF_LEN`/`CONTROLLER_OFFSET`, moved out of
  `client` so `client`/`replay` are pure FFI).

### Changed (breaking: `gputools-replay-hl` 0.2.0)

- `gputools_replay_hl::TextureDescriptor` is now the session-based descriptor
  (streamRef-keyed, read off the live texture). The offline manifest descriptor
  it used to name is re-exported as `OfflineTextureDescriptor`.
- The offline manifest path - `Capture::describe` / `textures_described` /
  `manifest_status` / `record_count`, `Descriptions`, `DescribedTexture`,
  `ManifestStatus`, `OfflineTextureDescriptor` - and its `gputrace-bundle`
  dependency are now behind the off-by-default `offline-manifest` feature. The
  default build is session-only and no longer depends on `gputrace-bundle`.
  In-session consumers migrate from `describe()` to `texture_descriptor()`.

## 0.1.1 - 2026-09-03

### Added

- `Capture::record_count()` (`gputools-replay-hl`): the manifest's index
  record count, an upper bound on the highest streamRef a fetch sweep needs to
  try. `None` when the manifest is absent or unparseable. Requires
  gputrace-bundle 0.1.1.

`gputools-replay-sys` and `gputools-replay` are version-bumped in lockstep;
they have no functional changes this release.

## 0.1.0 - 2026-09-03

### Added

- Initial release: raw FFI bindings (`gputools-replay-sys`), the safe
  in-process wrapper (`gputools-replay`), and the ergonomic domain layer
  (`gputools-replay-hl`) over Apple's private GPUToolsReplay framework.
