# Changelog

Notable changes per release. Dates are the publish date.

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
