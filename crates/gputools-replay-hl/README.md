# gputools-replay-hl

An ergonomic domain layer over [`gputools-replay`](https://crates.io/crates/gputools-replay),
the safe wrapper around Apple's **private** `GPUToolsReplay` framework. It adds
format-aware textures, typed buffers, and the descriptor join that reconciles
fetched resources against a capture's on-disk manifest (read via
[`gputrace-bundle`](https://crates.io/crates/gputrace-bundle)).

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
