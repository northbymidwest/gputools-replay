# Releasing

The three framework crates (`gputools-replay-sys`, `gputools-replay`,
`gputools-replay-hl`) release together, in that order, driven by
`.github/workflows/release.yml`. Every release is a `0.x` pre-release.

## Prerequisites (one-time)

- **A self-hosted macOS 27 (arm64) runner.** `cargo publish` runs a verify
  build that links the private framework, which no GitHub-hosted runner can do.
  The `publish` job targets `[self-hosted, macOS, ARM64]`.
- **`gputrace-bundle` published first.** `gputools-replay-hl` depends on it;
  at publish time hl's dependency resolves from crates.io, so the matching
  `gputrace-bundle` version must already be live (release it from its own repo).
- **crates.io trusted-publisher entries**, one per crate: owner `northbymidwest`,
  repo `gputools-replay`, workflow `release.yml`, environment `release`.
- **A `release` environment** in repo settings with a required reviewer, and
  restricted to `main`.

## By hand, before dispatching

1. Bump `version` in each of the three crates' `Cargo.toml` to the new version
   (they release in lockstep).
2. Remove the `publish = false` line from each of the three crates (it is the
   deliberate safety net that keeps them off crates.io until you mean it).
3. If `gputrace-bundle` is now published, switch hl's dependency from the
   `../gputrace-bundle` path+version form to a plain `version = "0.1"` (remove
   the path override in the workspace's `[workspace.dependencies]`).
4. Retitle the `## Unreleased` section of `CHANGELOG.md` to
   `## <version> - <YYYY-MM-DD>`.
5. Commit, push, and wait for CI to go green.

## Dispatch

Actions -> release -> Run workflow. Enter the version without a leading `v`.
Leave `dry_run` ticked for a rehearsal; untick it to publish.

- `preflight` (no approval) validates the version format, that every crate's
  manifest carries it and is no longer `publish = false`, that a non-empty
  `CHANGELOG.md` section exists, and that the newest CI run for the commit is
  green.
- `publish` pauses at the `release` environment's reviewer gate. On approval it
  exchanges an OIDC token for a short-lived crates.io token, publishes sys ->
  replay -> hl in order, then pushes the `v<version>` tag and creates a
  `--prerelease` GitHub Release from the changelog section.

`dry_run` defaults to true on purpose: forgetting to untick costs a re-run;
forgetting to tick would be an irreversible publish.
