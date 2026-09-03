# Security policy

## Supported versions

These crates are in a `0.x` series and every release is a pre-release. There is
no long-term support branch: a fix ships in a new release cut from `main`, and
older versions are not patched. If you hit a security issue, expect the fix in
the next release rather than a backport.

## What is in scope

These crates wrap Apple's private `GPUToolsReplay.framework` through an `unsafe`
FFI boundary and decode reverse-engineered capture data, so the interesting
failures are memory-safety and correctness bugs on that boundary:

- Memory-safety or soundness bugs in the FFI wrappers or in the
  fetch / playback / harvester decoding paths: out-of-bounds reads or writes,
  use-after-free, unsound `Send` / `Sync`, or type-confusion across the `objc2`
  boundary.
- Mis-decoded capture data presented to the caller as correct (a wrong value
  reported as a trustworthy one, not merely a value the crate declines to
  decode).
- The release and publishing path: the publishing workflow, its
  trusted-publishing configuration, or an archive / tag that does not match the
  source it claims to build from.

## What is not in scope

- Anything that requires a modified or hostile build of the private framework,
  or a modified or hostile OS. The trust boundary is Apple's shipping framework
  on a stock system.
- Behavior on unsupported macOS (older than macOS 27), where the crate refuses
  to build by design.
- Anything that can only be reproduced with a capture you cannot share. Without
  a repro there is nothing to fix; see below for what to send.

## Reporting a vulnerability

Please report privately through GitHub's private vulnerability reporting: open
the repository's **Security** tab and choose **Report a vulnerability**. Do not
open a public issue for a suspected vulnerability.

To let a fix happen quickly, include:

- the smallest repro you can manage;
- `rustc -Vv`;
- `sw_vers -productVersion`;
- the crate versions (or git SHA) involved;
- if it is shareable, the capture that triggers it.

This is a one-person project. Replies are best-effort and usually land within a
few days.
