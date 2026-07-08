# SDL3 audio backend, design

date: 2026-07-08

## goal

Replace both `lunar-audio` backends with SDL3, pulling `cubeb` and `cpal` out
entirely:

- **native** (`cfg(not(target_arch = "wasm32"))`): `cubeb` → SDL3 audio.
- **wasm32**: `cpal` → SDL3 audio, via a standalone emscripten-compiled
  sidecar module, mirroring the existing `lunar-plugin-physics-3d` jolt
  sidecar pattern.

Rationale (user's): SDL3 natively supports pipewire (the de-facto linux
audio standard), SDL3 natively supports wasm, the engine already depends on
SDL3 for windowing/input/gamepad. The one gap, no ASIO, is irrelevant.

Scope is a pure internal-crate swap: `lunar-audio` is not currently used by
any example or game code in this repo, so there's no external API-compat
risk beyond the crate's own public surface, which stays as close to
unchanged as possible.

## non-goals

- no change to `AudioSource` / `Mixer` / `PlaybackOptions` / decoding
  (symphonia-based decode path is untouched, backend-agnostic already)
- no latency tuning beyond what SDL3 gives by default (see risks)
- no pthreads/SharedArrayBuffer/COOP-COEP audio streaming on wasm32 — same
  call the project already made for the jolt sidecar
  (`plans/todo.md`, "wasm physics sidecar" section): single-threaded v1,
  threading deferred
- no shared `Sdl` context between `lunar-audio` and `bootstrap.rs`'s
  windowing/input setup (see native backend section)

## approach comparison: wasm32 streaming model

The one genuinely open fork is how the wasm32 sidecar feeds audio to SDL3
across the JS↔wasm boundary. Two real options:

**A. push, batched-per-frame (recommended).** A per-frame system calls
`Mixer::fill()` into a scratch buffer and tops up the sidecar's SDL3
`AudioStream` queue (`SDL_PutAudioStreamData`) to a target depth each frame.
No threads, no callback crossing the wasm↔wasm boundary. Mirrors the jolt
sidecar's existing batched-once-per-frame bridge exactly (same
`wasm_bindgen(inline_js=...)` shape, same "one call per frame, not one call
per unit-of-work" design). Matches the project's already-settled
single-threaded-sidecar precedent.

**B. pull, callback-via-pthreads.** SDL3's native callback model
(`AudioCallback`) ported faithfully: the sidecar runs a real audio callback
on an emscripten pthread, which would need to call back into the *main*
wasm module's `Mixer::fill()` on every device tick. This requires
`-pthread`/`SharedArrayBuffer` on both wasm modules and COOP/COEP response
headers on whatever serves the page. Rejected: this is exactly the
complexity `plans/todo.md` already deferred for the jolt sidecar, and it'd
be a bigger, riskier lift for audio glitches that a jitter buffer (option A)
already solves.

**C. no sidecar, cpal stays on wasm32.** Rejected per explicit user scope:
"pull cpal and cubeb entirely."

Going with **A**.

## native backend

Replace `crates/lunar-audio/src/backend/native.rs`'s `CubebBackend` with an
`Sdl3Backend`, keeping the exact same struct split the real `CubebBackend`
already has (verified against the current file): `Mixer` lives entirely
inside the callback (owned by the audio thread), `Sender` lives on the
outer backend struct (touched by `submit()` from the ECS thread), the two
never share state except through the channel.

```rust
struct MixerCallback {
    mixer: Mixer,
    scratch: Vec<f32>, // resized lazily, mirrors cubeb's `flat` buffer
}

impl sdl3::audio::AudioCallback<f32> for MixerCallback {
    fn callback(&mut self, stream: &mut sdl3::audio::AudioStream, requested: i32) {
        // requested is total interleaved samples (not frames): confirmed in
        // sdl3-rs's callback trampoline, `len / size_of::<Channel>()`.
        // mixer.fill() already takes a flat &mut [f32], so this drops
        // cubeb's StereoFrame conversion step entirely.
        let needed = requested.max(0) as usize;
        self.scratch.resize(needed, 0.0);
        self.mixer.fill(&mut self.scratch);
        stream.put_data_f32(&self.scratch).ok();
    }
}

pub struct Sdl3Backend {
    sender: Sender<Box<dyn AudioSource>>, // same crossbeam sender CubebBackend uses
    _stream: Sdl3StreamHandle, // unsafe impl Send/Sync, same justification as
                               // today's CubebHandle/StreamHandle
}

// wraps just the returned AudioStreamWithCallback<MixerCallback>, nothing
// else needed: traced the sdl3-rs Drop chain, AudioStreamOwner already
// holds `audio_subsystem: Option<AudioSubsystem>` internally (audio.rs:1083),
// and AudioSubsystem::new() "forgets" a cloned SdlDrop on first construction,
// only reclaiming it when the last instance drops (the `subsystem!` macro in
// sdl.rs). so the stream alone transitively keeps AudioSubsystem and the
// overall SDL_Init alive, no need to separately store `Sdl`/`AudioSubsystem`.
struct Sdl3StreamHandle(sdl3::audio::AudioStreamWithCallback<MixerCallback>);

impl AudioBackend for Sdl3Backend {
    fn submit(&self, source: Box<dyn AudioSource>) {
        let _ = self.sender.send(source);
    }
}
```

- `sdl3::init()?.audio()?.open_playback_stream(&spec, MixerCallback { .. })`
  opens the default playback device. `AudioSpec`'s fields are
  `Option<i32>`/`Option<AudioFormat>` (`None` = SDL's own device default), so
  the actual spec is `AudioSpec { freq: Some(SAMPLE_RATE as i32), channels:
  Some(2), format: Some(AudioFormat::F32LE) }`.
- **Must call `.resume()` on the returned stream.** Caught directly from the
  doc comment on `open_playback_stream_with_callback`: "The device begins
  paused, so you must call `stream.resume()` to start playback." Missing
  this is silent, no error, just no sound. This is the direct equivalent of
  cubeb's explicit `stream.start()?` call in the current `native.rs`, easy
  to drop by accident since SDL3's the one requiring an extra step here.
- Calls its own, independent `sdl3::init()` rather than sharing
  `bootstrap.rs`'s `Sdl` handle. **Verified in `sdl3-rs` source**
  (`src/sdl3/sdl.rs`): `Sdl::new()` hard-errors if called a second time from
  any thread other than the one that first called it (outside `cfg(test)`),
  so this only works because `AudioPlugin::build()` runs synchronously
  during `app.add_plugin(...)` in `bootstrap()`, the same thread that ran
  the original `sdl3::init()` there. Given that, the two `Sdl` instances
  share the same underlying `SDL_Init` refcount (`SDL_COUNT` static) and
  `AudioSubsystem` its own separate refcount (`AUDIO_COUNT`), so the second
  `init()` + `.audio()` just bumps counters, no double-init error. This is
  same-thread-only: if `AudioPlugin` were ever built from a spawned thread
  (e.g. some future async setup path) this would panic outside tests. Not a
  concern today, `GamePlugin::build()` calls are synchronous by
  construction, but worth flagging for whoever touches this later. Keeps
  `AudioPlugin` decoupled from `lunar-render`/`lunar-input`, no new
  cross-crate resource plumbing.
- pipewire comes for free: it's SDL3's default linux audio driver.
- `unsafe impl Send`/wrapper pattern around the device handle stays,
  same shape and same justification as today's `CubebHandle`.

**Rough edge, accepted:** cubeb's `StreamBuilder::latency(512)` had an
explicit frame-count hint; `sdl3-rs`'s `AudioSpec` has no equivalent, SDL3
manages its own buffering internally. `ponytail:` shipping with SDL3's
default buffering, revisit only if latency is ever measured as a problem
(SDL3 does expose a device-format readback post-open if tighter control is
needed later).

## wasm32 sidecar backend

Same file-for-file shape as `lunar-plugin-physics-3d`'s `SidecarBackend`
(`crates/lunar-plugin-physics-3d/src/sidecar.rs`, sibling `lunar-plugins`
repo), adjusted for a push/streaming API instead of a request/reply one.

**C sidecar API** (`crates/lunar-audio/sidecar/sidecar_api.h`):

```c
// batched C API exported by audio_sidecar.wasm.
// designed to cross the JS<->wasm boundary once per frame, pushing a chunk
// of interleaved f32 PCM, not once per sample.

int      audio_init(int freq, int channels);   // 0 on failure
void     audio_push(const float *data, int num_floats);
uint32_t audio_queued_bytes(void);
void     audio_shutdown(void);
```

No `audio_pause`/`audio_resume` in v1: nothing in this design ever calls
them (native's `AudioBackend` has no pause/resume either, `CubebBackend`
just runs continuously), so they'd be speculative API surface. Cut per
YAGNI, add back if a real pause/resume game-lifecycle hook shows up later.

Implementation (`sidecar_api.c`) is a thin wrapper over
`SDL_OpenAudioDeviceStream(SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, &spec, NULL,
NULL)` (no C callback: NULL callback means SDL just pulls whatever's been
queued via `SDL_PutAudioStreamData`, no pthread involved) plus
`SDL_GetAudioStreamQueued`/`SDL_DestroyAudioStream`. Same paused-by-default
behavior as the native side: `audio_init` must call the raw
`SDL_ResumeAudioStreamDevice` SDL call right after opening the stream (an
internal detail of `audio_init`, not the same thing as the now-cut public
`audio_resume` API), or `audio_sidecar.wasm` opens a stream that never
plays. Already present and working in the smoke-tested code below.
**Verified this session**: `audio_init` / `audio_push` / `audio_queued_bytes`
/ `audio_shutdown` (the `SDL_OpenAudioDeviceStream`/`SDL_PutAudioStreamData`/
`SDL_GetAudioStreamQueued`/`SDL_DestroyAudioStream` calls) compiled and
linked clean against emscripten's own SDL3 port (see build tooling
section).

**Rust bridge** (`crates/lunar-audio/src/backend/sidecar.rs`, renamed from
today's `web.rs` to match the physics module's naming):

```rust
#[wasm_bindgen(inline_js = r#"
export function audio_init(freq, channels) {
    return window.__audioSidecar._audio_init(freq, channels);
}
export function audio_push(ptr, num_floats) {
    if (num_floats === 0) return;
    const M = window.__audioSidecar;
    const bytes = num_floats * 4;
    const sidecar_ptr = M._malloc(bytes);
    new Uint8Array(M.HEAPU8.buffer, sidecar_ptr, bytes)
        .set(new Uint8Array(wasm.memory.buffer, ptr, bytes));
    M._audio_push(sidecar_ptr, num_floats);
    M._free(sidecar_ptr);
}
export function audio_queued_bytes() {
    return window.__audioSidecar._audio_queued_bytes();
}
export function audio_shutdown() {
    window.__audioSidecar._audio_shutdown();
}
"#)]
extern "C" {
    fn audio_init(freq: i32, channels: i32) -> i32;
    fn audio_push(ptr: u32, num_floats: i32);
    fn audio_queued_bytes() -> u32;
    fn audio_shutdown();
}

pub struct SidecarBackend {
    sender: Sender<Box<dyn AudioSource>>,
    mixer: Mixer,
    scratch: Vec<f32>,
    target_queued_bytes: u32, // jitter buffer depth, defaults to ~100ms:
                              // SAMPLE_RATE * channels * 4 bytes * 0.1
}

impl Drop for SidecarBackend {
    fn drop(&mut self) {
        audio_shutdown();
    }
}

impl AudioBackend for SidecarBackend {
    fn submit(&self, source: Box<dyn AudioSource>) {
        let _ = self.sender.send(source);
    }

    fn pump(&mut self) {
        let queued = audio_queued_bytes();
        if queued >= self.target_queued_bytes { return; }
        let want_floats = (self.target_queued_bytes - queued) as usize / 4;
        self.scratch.resize(want_floats, 0.0);
        self.mixer.fill(&mut self.scratch);
        audio_push(self.scratch.as_ptr() as u32, self.scratch.len() as i32);
    }
}
```

Still needs the `sender`/`Mixer` split even though wasm32 has no separate
audio thread: `submit` takes `&self` (any system can call it via
`Res<AudioPlayer>`, no exclusive access), `Mixer::fill()` takes `&mut self`.
The channel is what bridges that gap, same reason `CubebBackend` needs it,
not a leftover from copying the native shape.

`window.__audioSidecar` is loaded by the HTML/JS harness before the main
wasm module inits, exactly like `window.__jolt` today (see build tooling
section for the `run_wasm.go` change).

## `backend/mod.rs` changes

Re-verified against the current file. This is the actual swap point, and
needs more than the trait addition below:

```rust
//! platform audio backends: sdl3 native, sdl3-emscripten-sidecar on wasm32.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod sidecar;

#[cfg(not(target_arch = "wasm32"))]
pub use native::Sdl3Backend as PlatformBackend;
#[cfg(target_arch = "wasm32")]
pub use sidecar::SidecarBackend as PlatformBackend;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn init() -> Result<PlatformBackend, String> {
    native::Sdl3Backend::new().map_err(|e| e.to_string())
}
#[cfg(target_arch = "wasm32")]
pub(crate) fn init() -> Result<PlatformBackend, String> {
    sidecar::SidecarBackend::new()
}
```

Top-of-file doc comment (`"cubeb on native, cpal/webaudio on wasm32"`)
updated to match. `native`'s `init()` body keeps the same
`.map_err(|e| e.to_string())` shape unchanged: verified `sdl3::Error`
implements `Display` (`sdl3-rs`'s `sdl.rs:19`, wraps a `String`), same as
`cubeb::Error` today. `sidecar`'s `init()` body stays a direct call with no
`map_err`, matching `CpalBackend::new() -> Result<Self, String>`'s existing
signature today: `SidecarBackend::new()` should return `Result<Self, String>`
too, not introduce a different error type.

## `AudioBackend` trait change

One addition, no-op by default, so the public `AudioPlayer` API
(`play`/`play_source`) is untouched:

```rust
pub trait AudioBackend: Send + 'static {
    fn submit(&self, source: Box<dyn AudioSource>);
    fn pump(&mut self) {} // native: no-op, OS thread already pulls on demand
}
```

`AudioPlugin::build()` registers a plain per-frame system, same
registration path (`app.add_system(...)`) every other plugin already uses:

```rust
fn pump_audio_backend(mut player: ResMut<AudioPlayer>) {
    player.backend.0.pump();
}
```

Defined in `plugin.rs` alongside `AudioPlayer`, so it can reach the private
`backend` field directly, no new public accessor needed. Free on native
(empty call), does the jitter-buffer top-up on wasm32.

**Registration must stay inside the `Ok(backend)` branch.** Verified in the
current `plugin.rs`: `AudioPlugin::build()` only calls
`app.insert_resource(AudioPlayer::new(backend))` when `backend::init()`
succeeds, it logs and skips on `Err` (today's graceful degrade path when no
audio device exists). `add_system(pump_audio_backend)` has to go inside that
same `Ok` arm, a `ResMut<AudioPlayer>` system registered unconditionally
would panic on missing resource the moment init fails. This is the one
place the new per-frame system has to be threaded through existing control
flow rather than just appended.

## build tooling

**No vendored SDL3 checkout needed.** Emscripten ships its own SDL3 port
(`/usr/lib/emscripten/tools/ports/sdl3.py`, pins upstream `release-3.4.2`),
fetched and cached automatically the first time `-sUSE_SDL=3` is passed to
`emcc`. This is simpler than the jolt sidecar's `JOLTC_DIR`
vendor-a-local-checkout pattern, because unlike JoltPhysics, SDL3 already
has first-class emscripten port support. Verified end-to-end this session:
a minimal `sidecar_api.c` calling `SDL_OpenAudioDeviceStream` /
`SDL_PutAudioStreamData` compiled and linked cleanly with plain
`emcmake cmake` + `-sUSE_SDL=3`.

`crates/lunar-audio/sidecar/CMakeLists.txt`:

```cmake
cmake_minimum_required(VERSION 3.20)
project(audio_sidecar C)

add_executable(audio_sidecar sidecar_api.c)
set_target_properties(audio_sidecar PROPERTIES SUFFIX ".js")

target_compile_options(audio_sidecar PRIVATE "-sUSE_SDL=3" "-O3")
target_link_options(audio_sidecar PRIVATE
    "-sUSE_SDL=3"
    "-O3"
    "-sMODULARIZE=1"
    "-sEXPORT_NAME=createAudioSidecarModule"
    "-sEXPORTED_FUNCTIONS=_audio_init,_audio_push,_audio_queued_bytes,_audio_shutdown,_malloc,_free"
    "-sEXPORTED_RUNTIME_METHODS=HEAPU8"
    "-sALLOW_MEMORY_GROWTH=1"
    "-sNO_EXIT_RUNTIME=1"
    "--no-entry"
)
```

`crates/lunar-audio/sidecar/build.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="${SCRIPT_DIR}/build"
OUT_DIR="${1:-${SCRIPT_DIR}/dist}"

command -v emcmake >/dev/null || { echo "error: emcmake not on PATH (install emscripten)"; exit 1; }

emcmake cmake -B "${BUILD_DIR}" "${SCRIPT_DIR}" -DCMAKE_BUILD_TYPE=Release
cmake --build "${BUILD_DIR}" --config Release -j "$(nproc)"

mkdir -p "${OUT_DIR}"
cp "${BUILD_DIR}/audio_sidecar.js"   "${OUT_DIR}/"
cp "${BUILD_DIR}/audio_sidecar.wasm" "${OUT_DIR}/"
echo "done: ${OUT_DIR}/audio_sidecar.{js,wasm}"
```

`ponytail:` no `EMSDK` env-var gate like jolt's `build.sh` has. This repo's
emscripten install is the Arch `emscripten`/`binaryen` system packages
(`/etc/profile.d/emscripten.sh` puts `emcc`/`emcmake`/`emconfigure` on PATH
globally), not an `emsdk`-installer checkout, so there's no `EMSDK` var to
require. If a future dev uses the emsdk installer instead, `emcmake` still
works the same, just sourced from `emsdk_env.sh` first.

`crates/lunar-audio/sidecar/dist/` (and `sidecar/build/`, CMake's own build
directory) need actually adding to `.gitignore`, they're not covered by
anything already there. Checked the real `.gitignore`: it has `/dist/`
(root-anchored, only matches a top-level `dist/`, not this nested one) and
no generic `build/` pattern at all, only `**/target/` for rust and
`**/bin/`/`**/obj/`/`**/publish/` for dotnet. Add:

```
crates/lunar-audio/sidecar/build/
crates/lunar-audio/sidecar/dist/
```

Lives in-repo (not a sibling repo like jolt's `dist`), since there's no
external upstream C++ project to version separately, SDL3 is already an
ordinary crates.io dependency for the native side.

**`scripts/run_wasm.go` change:** generalize the single `hasSidecar`
bool/block (lines ~81-106) into a list of sidecar descriptors, so both the
jolt and audio sidecars can be present independently:

```go
type sidecarModule struct {
    distDir      string
    jsFile       string
    wasmFile     string
    globalName   string // e.g. "__jolt", "__audioSidecar"
    factoryName  string // e.g. "createJoltModule", "createAudioSidecarModule"
}

sidecars := []sidecarModule{
    {filepath.Join(filepath.Dir(root), "jolt", "jolt-rust", "sidecar", "dist"),
        "jolt_sidecar.js", "jolt_sidecar.wasm", "__jolt", "createJoltModule"},
    {filepath.Join(root, "crates", "lunar-audio", "sidecar", "dist"),
        "audio_sidecar.js", "audio_sidecar.wasm", "__audioSidecar", "createAudioSidecarModule"},
}
```

Each present sidecar (both files exist) gets copied into `tmpDir` and gets
an `import ... ; window.__x = await createXModule();` line prepended to
`moduleScript`, in list order, all before `import init from './%s.js'`.
Order between the two sidecars doesn't matter, both just need to load
before the main module.

## Cargo.toml changes

`crates/lunar-audio/Cargo.toml`:

- drop `cubeb = "0.34"` and `cpal = { version = "0.15", ... }` entirely
- non-wasm32 target block: add `sdl3 = { workspace = true }`. Already
  carries `build-from-source-static` from the workspace-level dependency
  entry (`Cargo.toml:51`), no need to redeclare it. Doesn't need `lunar`'s
  extra `raw-window-handle` feature (that's for windowing, irrelevant here),
  just the base audio subsystem.
- wasm32 target block: add `wasm-bindgen = "0.2"` (matches
  `lunar-plugin-physics-3d`'s wasm32 dep shape)
- update `description` field: "audio system: AudioSource trait, mixer,
  symphonia decoding, sdl3 native + emscripten-sidecar backends"

Doc comments in `lib.rs` (backend descriptions) and `plugin.rs`'s
`backend_name()` (`"cubeb"` / `"cpal/webaudio"` → `"sdl3"` /
`"sdl3 (emscripten sidecar)"`) get updated to match. Also `plugin.rs`'s
`SyncBackend` comment (verified in the current file, line 11): `"it holds a
Sender<Box<dyn AudioSource>> and CubebHandle"` names `CubebHandle`
explicitly, that type is gone after this rewrite (renamed
`Sdl3StreamHandle`, only exists in `native.rs` not wasm32), so this comment
goes stale if not updated alongside `backend_name()`.

**A third location, outside `lunar-audio` entirely**: `crates/lunar/src/lib.rs:50`
(the `lunar-engine` facade crate) has a features table with
`| `audio` | symphonia decoding, cubeb (native) / WebAudio (WASM) |`. Found
by grepping the whole workspace for `AudioBackend`/`AudioPlugin`/`AudioPlayer`
outside `crates/lunar-audio/`, this is the only other hit (`bindings/c` and
`tests/api_seal` have none). Update this row to name SDL3 instead, since
the scope claim ("no external API-compat risk beyond the crate's own public
surface") is about the *code* surface, not prose describing it elsewhere.

## testing

- native: no existing example currently exercises `lunar-audio`. Plan is a
  throwaway local smoke test (play a decoded tone through `Sdl3Backend`,
  confirm audible output on this machine's real speakers/pipewire), deleted
  before commit. `cargo build`/`cargo clippy` cover the rest.
- wasm32 sidecar: `sidecar_api.c` compiles/links against emscripten's SDL3
  port, verified this session. Full runtime test (load in a real browser,
  confirm audio plays through the bridge) happens during implementation via
  `scripts/run_wasm.go`, same as any other wasm-target change in this repo.
- no CI audio-device test added: matches the crate's current lack of
  automated backend tests, and there's no headless-audio-device precedent
  in this repo the way there is for headless GPU (lavapipe).

## risks / follow-ups

- SDL3's emscripten port is upstream-flagged experimental
  (`-Wexperimental` warning on build). Functional as verified, but a future
  emscripten/SDL upgrade could shift behavior. Not a blocker: same
  experimental-but-works status as plenty of this project's other bleeding
  edge deps.
- version drift: this workspace's `Cargo.lock` actually pins
  `sdl3-sys 0.6.5+SDL-3.4.8` (checked directly, not the newer 3.4.12 that
  happens to sit in this machine's shared cargo registry cache from some
  other project) for native, while the wasm sidecar's emscripten port pins
  SDL `3.4.2`. Two different patch builds, plus the system's pacman `sdl3`
  package (3.4.12) is irrelevant either way since `sdl3-rs`'s
  `build-from-source-static` feature compiles its own vendored source
  rather than linking the system lib. Not a problem: the native binary and
  the wasm module are entirely separate compiled artifacts, never linked
  together, both just need to implement the same stable `SDL_AudioStream`
  API surface.
- sidecar wasm size: **corrected after checking, this was wrong in an
  earlier draft.** `run_wasm.go`'s `wasm-opt -O3` pass (lines 60-79) runs
  only on the main module's `_bg.wasm`, and completes entirely *before* the
  sidecar-copy block even starts (line 81). Sidecar `.wasm` files are
  copied byte-for-byte from `sidecar/dist/`, `run_wasm.go` never touches
  them. Any sidecar shrinking has to happen in `sidecar/build.sh`/
  `CMakeLists.txt` itself. Turns out this is already partially covered:
  `-DCMAKE_BUILD_TYPE=Release` (already in `build.sh`) injects
  `CMAKE_C_FLAGS_RELEASE=-O3 -DNDEBUG` automatically, verified by diffing
  against an explicit `-O0` build (819KB vs. 690KB for the smoke test, so
  Release's implicit `-O3` was already doing real work, the 690KB figure
  reported earlier in this doc was an *already-optimized* number, not a raw
  one as previously stated). The explicit `-O3` in `target_compile_options`/
  `target_link_options` is therefore redundant with `CMAKE_BUILD_TYPE`, kept
  for explicitness so a reader doesn't have to know that CMake default to
  see where the optimization comes from. If the sidecar ever needs to be
  smaller, the lever is `-Oz` instead of `-O3` in the sidecar's own
  `CMakeLists.txt`, not anything in `run_wasm.go`.
- latency: SDL3's default device buffering (native) and the jitter-buffer
  depth (wasm32 `pump()`) are both unmeasured/untuned in this pass. Ship
  first, tune only if a real latency complaint shows up.
