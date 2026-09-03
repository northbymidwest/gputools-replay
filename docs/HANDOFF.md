# `gputools-replay` - design handoff

> This is the distilled reverse-engineering record behind these crates: the
> bootstrap and fetch facts, each with the method that established it. The
> substrate it describes is built and merged, and the RE campaign is done.

You are designing and building a Rust crate that wraps Apple's **private**
`GPUToolsReplay` framework, so that `.gputrace` captures can be driven
programmatically.

There is a prior implementation (a `ktx2-fetch` spike). **It is evidence, not
a template.** It was a first draft written
to answer one question - can we get lossless texture bytes out of a capture - 
and its structure is shaped by that goal and by the order things were
discovered, not by what a reusable library should look like. Read it to check
facts. Do not port it.

This document is the distilled result of about 100 commits of reverse
engineering. Everything in the "Established" section was measured or read off
disassembly, and each item says how, so you can re-check it rather than trust
it. Everything in "Open" is genuinely unknown - do not let it become
"probably".

---

## 1. What the framework is

`GPUToolsReplay` - a private framework, new in macOS 27. No headers, no
documentation. It exists in the dyld shared cache; you link it via the SDK stub
at `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/PrivateFrameworks/`.
It is the engine behind Xcode's GPU debugger and the `gpudebug` CLI.

It also needs APR (Apache Portable Runtime), `libapr-1.0.dylib`, because its
bootstrap wants an APR memory pool.

Linking is three lines in `build.rs` - no `dlopen`, no `libc`:

```rust
println!("cargo:rustc-link-search=framework=/System/Library/PrivateFrameworks");
println!("cargo:rustc-link-lib=framework=GPUToolsReplay");
println!("cargo:rustc-link-lib=dylib=apr-1.0");
```

---

## 2. Established facts

### 2.1 The unlock - without this nothing works

```
MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0
```

**Mandatory.** With `lockParameterBufferSizeToMax = 1` (the default), the replay
device's command-queue creation returns nil in an unentitled process and every
fetch fails. Found by swizzling the ObjC method and observing the nil.

`std::env::set_var` is `unsafe` in edition 2024 and sound only while the process
is single-threaded. The prior tool resolved this by having the **binary set it**
as the first statement of `main`, and the **library verify** it rather than set
it. That split is worth keeping: a library cannot guarantee the precondition,
but it can refuse to proceed without it. Consider whether your crate should
verify-and-error, or expose an explicit `unsafe fn` that documents the
requirement.

### 2.2 Bootstrap sequence

Read off `GPUToolsReplayService.xpc`'s own binary, which does exactly this:

```
apr_initialize()
apr_pool_create_ex(&pool, NULL, NULL, NULL)
GTMTLReplayController_init(pool)
GTMTLReplayClient_init(&client, pool)     // caller-allocated, see 2.3
GTMTLReplayErrorHandling_initWithObserver(observer)   // BEFORE load:
[[GTMTLReplayService alloc] initWithContext:client]
[service load:<NSURL> error:&err]
```

C signatures:

```rust
fn apr_initialize() -> c_int;
fn apr_pool_create_ex(pool: *mut *mut AprPool, parent: *mut AprPool,
                      abort_fn: *mut c_void, allocator: *mut c_void) -> c_int;
fn GTMTLReplayController_init(pool: *mut AprPool) -> *mut GTMTLReplayController;
fn GTMTLReplayClient_init(out: *mut GTMTLReplayClient, pool: *mut AprPool);
fn GTMTLReplayErrorHandling_initWithObserver(observer: *mut AnyObject);
```

Each argument count was confirmed from the callee prologue (e.g.
`GTMTLReplayClient_init` never reads x2/x3/x4). `apr_pool_create_ex`'s last two
arguments are `apr_abortfunc_t` and `apr_allocator_t*`; both are passed NULL and
neither type was established, so neither was given one.

Three traps:

- **`load:` takes an `NSURL`, not a path string and not a request object.**
  Passing a `GTReplayLoadRequest` raises
  `-[GTReplayLoadRequest scheme]: unrecognized selector`.
- **Register the error observer before `load:`** or failures are silent. The
  observer just needs to respond to `-notifyError:`.
- **`load:` returning YES only means its dispatched block ran.** It is not a
  success signal. You must consult the error observer.

### 2.3 `GTMTLReplayClient` is caller-allocated, and its size is derivable

`GTMTLReplayClient_init` writes into a buffer you supply. **312 bytes (0x138),
alignment 8.**

How that was derived, and it matters that you can redo it:

1. `-[GTMTLReplayService initWithContext:]`'s ObjC type encoding contains the
   struct's complete layout. Read it from the **live runtime** via
   `class_getInstanceMethod` + `method_getTypeEncoding`. Do not copy it from any
   document - an earlier transcription in the old repo was truncated and elided
   176 bytes, which would have produced a 136-byte buffer against a framework
   that writes to 0x12c.
2. `NSGetSizeAndAlignment` **refuses it** - `NSInvalidArgumentException:
   unsupported type encoding spec 'b'` on the `b1b1b1b1b28` bitfield run.
3. So: transcribe the encoding into a C struct, assert `@encode()` of the
   transcription is byte-for-byte the runtime's string, then take `sizeof`.
   Gives 312/8.
4. Cross-check by poisoning a large buffer and running the whole lifecycle: all
   writes landed in `[0x00, 0x12c]`, nothing at or past 0x138.

Two subtleties worth preserving:

- `@encode()` does **not** record a bitfield's declared storage type. Rebuilding
  the struct with the run declared `unsigned int` / `unsigned long long` / `int`
  / `long` gives 312/8 in all four cases, so the ambiguity is harmless - but it
  needs checking, not assuming.
- `@encode()` also does not record `aligned`/`packed` attributes. An
  over-aligned member could enlarge the real struct while encoding identically.
  This is **the one thing still assumed**. What constrains it: observed writes
  land at exactly the predicted offsets out to 0x128.

**Strongly recommended:** carry over the idea of a test that re-reads the
encoding from the runtime on every run and fails if it differs from the
recorded one. That converts "the layout is what we think" from a comment into a
checked invariant, so an OS update breaks the build loudly instead of silently
corrupting a heap. For a crate shipped to other people this is close to
mandatory.

### 2.4 Fetching

Classes: `GTReplayFetchTexture`, `GTReplayRequestBatch`.

```
fetch = [[GTReplayFetchTexture alloc] init]
  -setStreamRef:  (uint64)      v24@0:8Q16
  -setSize:       GTSize{u64 w,h,d}          v40@0:8{GTSize=QQQ}16
  -setRegion:     GTRegion{GTPoint3D{u64 x,y,z}, GTSize}
  -setPlane:      (uint32)
  -setDepth:      (uint32)   <-- must be 1
batch = [[GTReplayRequestBatch alloc] init]
  -setRequests:   NSArray
  -setCompletionHandler: block
[service fetch:batch]
```

`GTReplayFetchTexture` is fully described by the runtime (instanceSize 128) with
`streamRef`, `size`, `plane`, `slice`, `level`, `depth`, `region`,
`resolveMultisampleTexture`, `dispatchUID`.

Facts that cost real time to find:

- **`depth = 0` returns nothing.** A texture has at least one slice. This is the
  single most expensive mistake made on the prior project: an early probe zeroed
  *every* field including `depth`, concluded "a zero region returns an empty
  payload", and that wrong conclusion justified an entire subprocess dependency
  on `gpudebug` for weeks. Always send `depth = 1`.
- **A zero `width`/`height` region returns each texture at its NATURAL size**,
  unresampled. This is how you get real pixels.
- **A non-zero region RESAMPLES.** It scales the texture to fit, preserving
  aspect ratio - it is not a crop, and it upscales as well as downscales, so no
  request size ever "caps out" to reveal the natural size.
- **`streamRef`s are sparse.** Nothing like `0..n`. Callers sweep a range and
  keep whatever answers. A 2000-ref sweep on a real capture returned 182
  records.
- Never `waitUntilCompleted` - pump the runloop. The completion block arrives on
  a thread you do not own.

### 2.5 The reply format

The response is an `NSKeyedArchiver` binary plist with three keys:

- `unknown` - an NSArray. **Its meaning is not established.** It is empty in
  every reply measured, including sweeps where 1,818 of 2,000 refs went
  unanswered, so it is *not* a list of unresolved requests despite an earlier
  guess saying so. Do not repeat that claim.
- `info` - a descriptor table, **80 bytes per texture**
- `data` - concatenated raw pixels

Mapped `info` offsets (11 further offsets are unmapped; they are either zero or
the constant `0x10000` in every record observed):

| offset | field |
| --- | --- |
| 0x00 | streamRef (u32) |
| 0x08 | resourceIndex (u32) |
| 0x18 | payload offset into `data` (u32) |
| 0x1c | payload size (u32) |
| 0x30 | width (u32) |
| 0x34 | height (u16) |
| 0x36 | depth (u16) |
| 0x38 | MTLPixelFormat (u32) |
| 0x40 | bytesPerRow (u32) |
| 0x44 | bytesPerImage (u32) |

**`resourceIndex` is not unique per record.** A real 182-record reply had only
180 distinct values - the replayer can reach one resource through more than one
streamRef. The identity of a *fetched record* is `(streamRef, plane)`. The prior
tool shipped a silent-overwrite bug because it keyed filenames on
`resourceIndex`.

**At plane 0, the reply's streamRef matches the requested one.** At plane 1 it
did not (see Open questions). So treat the field's meaning as confirmed only for
plane 0.

### 2.6 Observed pixel formats

Across the captures available: `MTLPixelFormat` 10 (R8Unorm), 70 (RGBA8Unorm),
80 (BGRA8Unorm), 125 (RGBA32Float). A 182-record capture was
8 / 2 / 162 / 10 of those. No multi-planar format has ever been observed.

---

## 3. Open questions - do not let these become assumptions

- **What `plane:` selects is unknown.** Measured: requesting plane 1 of a
  single-plane `BGRA8Unorm` resource returned pixel data byte-for-byte
  *identical* to that resource's plane 0, under a streamRef outside the swept
  range. At least four readings fit (the field is ignored; out-of-range planes
  fall back; it selects something other than a Metal plane; the reply's ref
  field is not the request's). **It cannot be resolved without a capture
  containing a genuinely multi-planar texture** - none is available. If you can
  produce one (a YCbCr video frame), the experiment is short and decisive.
- **`level:` and `slice:` are unverified.** Never exercised.
- **Coverage is incomplete and unexplained.** On one capture `gpudebug` lists 7
  textures and only 4 ever answer the replayer (resourceIndex 108, 109, 111 - 
  including a `CAMetalLayer Display Drawable` - never do). Separately, the same
  sweep returns 182 textures at small regions and 180 at large ones. A
  *hypothesis worth testing, not a finding*: the fixture has exactly 182 records
  over 180 distinct resourceIndexes, so the two that "drop out" may be the
  duplicates collapsing.
- **`GTMTLReplayClient_preferDevice`'s argument is unestablished.** Two
  candidates segfault.
- **The capture-bundle shape check is inferred.** `load:` **SIGSEGVs** on a
  missing path or a non-capture directory rather than returning an error, so the
  prior tool validated that an `index` and a `metadata` entry exist first. All
  available captures have both; nothing documents that as the actual format
  requirement. **Your crate must guard this** - a library that segfaults on a
  bad path is not shippable.

---

## 4. Operational constraints (these will bite you)

- **Access must be serialised.** One replay session at a time, machine-wide.
- **One session per process.** A second bootstrap in the same process aborts
  (exit 132).
- **An interrupted run orphans a session and locks the replayer for TWO HOURS.**
  Recovery: `gpudebug --terminate all` then `pkill -9 -f GPUToolsReplayService`.
  Check `pgrep -f GPUToolsReplayService` before and after every run.
- Latency ranges from ~27 seconds to 20+ minutes. Do not assume a slow run hung.
  This range is for large third-party captures; the `fixture-apps/` suite
  answers in well under a second.
- Any test touching the replayer should be behind a feature flag (the prior repo
  used `--features oracle`) and run `--test-threads=1`.

---

## 5. What to take from the prior implementation, and what to leave

**Take the discipline, not the design.**

Worth carrying over:

- The binding rule the project ran on: *a component that cannot supply a value
  returns an error naming it - never a default, never a guess, never a
  neighbour's value reused.* Almost every real defect found on that project was
  a violation of it.
- *When you write MEASURED, say HOW.* Every claim above states its method for
  exactly this reason.
- The runtime-encoding regression test (2.3).
- Confining `unsafe` to one module with a `#![deny(unsafe_code)]` crate root
  everywhere else.
- Verifying the env var rather than setting it inside library code.

Worth reconsidering from scratch - these are first-draft shapes, not good ones:

- **The session/fetch API.** The old `Session::open` + `fetch(&[FetchRequest])`
  is shaped around one batch sweep. A library probably wants a clearer
  separation between "a loaded capture", "a query", and "a decoded result", and
  should think about whether `open` is fallible-but-crashy (it currently guards
  a SIGSEGV by inspecting the bundle first).
- **Record parsing.** The old code hand-walks `$objects` by UID because only
  three keys were needed. A general crate may want a real unarchiver, and should
  decide whether unmapped fields are exposed, preserved, or dropped.
- **Ownership and leaks.** The prior module deliberately leaks the client
  buffer, service, batch and token, on the reasoning that the framework retains
  them and nothing should be freed under it. That is defensible for a
  short-lived CLI and probably wrong for a library that might open and close
  sessions. This needs real thought, not a port.
- **Error types.** They accreted per-task. Design them once.
- **Whether KTX2 belongs here at all.** It does not - that is a consumer's
  concern. Keep this crate about the framework.

Suggested shape, which you should feel free to reject: a `-sys` crate holding
the FFI, the struct size, and the encoding guard; and a safe crate on top. The
prior project's `src/replay.rs` is 1,181 lines and is the only part relevant to
you; the other ~1,400 lines are KTX2 writing, manifests and CLI.

---

## 6. A caution about publishing

If this becomes a published crate, be honest in the README about what it is:
a wrapper around a private framework on an OS released weeks ago, with
signatures read from disassembly, validated on one machine. The encoding
regression test is what makes that defensible rather than reckless - it turns an
OS layout change into a build failure instead of memory corruption. Do not ship
without it.
