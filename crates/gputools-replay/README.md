# gputools-replay

A safe, in-process Rust wrapper around Apple's **private** `GPUToolsReplay`
framework. It drives a `.gputrace` capture programmatically: a
one-session-per-process model, texture / buffer / heap / pipeline-binaries /
acceleration-structure fetch, playback, and harvester decoding, with typed
results that preserve every field. Only `Session::configure_env` is `unsafe`
(an opt-in for non-default replayer config); every other public fn is safe.

Built on [`gputools-replay-sys`](https://crates.io/crates/gputools-replay-sys).
For an ergonomic, format-aware layer on top, see
[`gputools-replay-hl`](https://crates.io/crates/gputools-replay-hl).

> **Warning:** this links a private, undocumented Apple framework and can break
> at any time, whether from errors in the reverse engineering or from Apple
> changing the framework or the `.gputrace` format. Treat it as experimental,
> pin an exact version, and re-validate on every macOS update. See the
> [workspace README](https://github.com/northbymidwest/gputools-replay).

## macOS version

macOS 27 by default (the `macos27` feature); build `--no-default-features` for
macOS 26 (which lacks acceleration-structure fetch).

## License

`0BSD`. See [LICENSE](LICENSE).
