# security audit

date: 2026-07-16
concern: security (hostile assets, ffi boundary, dependency hygiene, unsafe soundness)

method: multi-agent fan-out (one finder agent per lens), location dedup, then every
surviving finding was handed to an independent adversarial verifier whose default was
to refute it — only findings that held up under the verifier's own reading of the code
are listed below. read-only audit; no source was modified. this doc was transcribed by
the orchestrator from the verified finding set after the workflow's writer stage was
cut off by a subagent model-credit limit — the findings and verdicts are the agents', unedited.

scope: hostile asset files, the plugin/ffi/coreclr boundary, dependency hygiene, and soundness of existing unsafe.

**17 confirmed findings** (6 high, 8 medium, 3 low).

discovery stats: 36 raw findings from 5 lenses (+0 gap follow-ups), 35 after dedup, 17 confirmed, 2 refuted.

every finding carries: id, location (file:line), impact, effort (S/M/L), and the
verified claim (verifier-corrected wording where the skeptic adjusted it). ids are
assigned in severity order and are stable references for the phase-2 backlog synthesis.

---

## sec-01 — bindings/c/src/lib.rs:15 (module doc), bindings/c/src/lib.rs:493-545 (lunar_component_get / lunar_component_get_mut)

- **impact:** high
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

The documented validity contract for component pointers returned by lunar_component_get[_mut] ("valid until the current system callback returns") is unsound: any structural world mutation made through other lunar_* calls within the same callback (spawn, despawn, component insert/remove) can move or reallocate bevy_ecs Table storage, dangling the pointer while the caller is still inside its documented validity window.

## sec-02 — crates/lunar-assets/src/lib.rs:799-801

- **impact:** high
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

BctexLoader::load sizes the base texture region with unchecked u32 arithmetic and then slices the input at bytes[16..16+base_size] with no bounds check, so a truncated or large-dimension .bctex asset triggers an out-of-bounds slice panic (process abort under panic=abort).

## sec-03 — crates/lunar-atlas/src/manifest.rs:131-132

- **impact:** high
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

AtlasManifest::read_from pre-allocates a HashMap with capacity taken directly from the untrusted 32-bit region_count, so an 18-byte header can force a multi-hundred-GB allocation and abort the process.

## sec-04 — crates/lunar-bsp/src/level.rs:120-136

- **impact:** high
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

BspLevel::camera_leaf walks the BSP tree trusting file-supplied child indices with neither a bounds check nor a cycle/depth guard, so a structurally-valid but malicious level blob causes either an out-of-bounds index panic or an infinite-loop hang every frame.

## sec-05 — crates/lunar-dotnet-host/src/lib.rs:40-61,140,181-194,208-211

- **impact:** high
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

The hostfxr bindings pass every char_t* argument as narrow UTF-8 c_char, but on Windows hostfxr's char_t is wchar_t (UTF-16), so on Windows the runtimeconfig/assembly/type/method paths are misinterpreted, breaking the CoreCLR path and causing an out-of-bounds read when the runtime scans a narrow, single-NUL-terminated buffer looking for a 16-bit NUL.

## sec-06 — crates/lunar-image/src/format.rs:110-126 (decode.rs:141-148)

- **impact:** high
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

Header::parse applies no upper bound to width/height, and decode passes width*height*4 as the zstd decompression capacity. Because lunar-image pins zstd with default-features=false (the `experimental` feature disabled), Decompressor::upper_bound is compiled out and always returns None, so zstd::bulk::decompress unconditionally performs a Vec::with_capacity(width*height*4) BEFORE reading the compressed payload — the attacker does not even need a frame lacking a declared content size. A ~40-byte crafted .li with width=height=65535 forces a ~17.18 GB up-front allocation; under the release panic=abort profile the failing allocation aborts the process. This is an unauthenticated denial-of-service (OOM/process-kill on hostile asset load via LiTextureLoader), not memory corruption. Fix: clamp width*height against a sane maximum (and/or the compressed input size) in Header::parse or decode before allocating.

## sec-07 — bindings/c/src/lib.rs:444

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

lunar_component_insert (bindings/c/src/lib.rs:444) builds NonNull::new_unchecked(data) with no null check, so a null `data` from a C/plugin caller is immediate UB despite the function otherwise defensively no-op-warning on every other invalid input (unknown component id, size mismatch, dead entity) and despite every sibling pointer-taking export (lines 1460, 1498, 1540, 1630, 1669, 1706, 1753, 1779, 1805, 1831) guarding with is_null(). The Safety doc does declare `data` non-null, but that contract cannot bind foreign callers at a C ABI; replacing with NonNull::new + warn-and-return matches the function's own posture at zero cost. The shipped dotnet wrapper cannot pass null, so exposure is limited to third-party C/plugin callers — the boundary this audit targets.

## sec-08 — bindings/c/src/lib.rs:673-681 (lunar_query_foreach)

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

lunar_query_foreach calls slice::from_raw_parts on the include/exclude pointers without a null check, but its own safety documentation invites null pointers when the count is 0 — from_raw_parts(null, 0) violates the stdlib safety contract (data must be non-null and aligned even for zero-length slices), i.e. documented-contract-compliant callers hit library UB, and on nightly dev builds a non-null debug assertion aborts the process.

## sec-09 — bindings/c/src/lib.rs:674,679

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

lunar_query_foreach calls slice::from_raw_parts(include/exclude, count) unconditionally, but its own safety doc says the pointer 'may be null when its count is 0', and slice::from_raw_parts requires a non-null, aligned pointer even for zero-length slices, so a documented-legal (NULL, 0) call from a C consumer is instant undefined behavior.

## sec-10 — crates/lunar-3d/src/scene_format_3d.rs:518-528

- **impact:** medium
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

SceneLoader3d::spawn_internal (scene_format_3d.rs:518-528) and the 2D SceneLoader::spawn_scene_internal (scene_format.rs:461-482) recurse into registry-resolved sub-scenes with no cycle detection or depth limit, so a scene registry containing a self-reference or cycle causes unbounded recursion and stack overflow (process abort, DoS), and even acyclic nested sub-scenes permit exponential entity expansion. Currently no in-tree caller passes a scene registry (AdvancedSceneLoader, the world manifest, and the FFI surface all pass None), so this is reachable only through the public spawn_scene/load_and_spawn API when an embedding game supplies a registry built from untrusted scene files — the documented intended usage of the sub_scene field.

## sec-11 — crates/lunar-audio/src/decoder.rs:199-201

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

resample_stereo (crates/lunar-audio/src/decoder.rs:199-201) sizes its output from the file-declared source_rate, guarded only against 0 and the exact-match case (decoder.rs:183). A hostile WAV can declare sample_rate=1 (symphonia's RIFF reader passes the raw u32 through with no validation), making out_frames = frame_count * 48000 — a ~48000x amplification — so Vec::with_capacity(out_frames*2) attempts a multi-GB-to-hundreds-of-GB allocation from a sub-MB file. The allocation failure invokes Rust's alloc-error handler and aborts the process: a denial-of-service (process kill under release panic=abort), not memory corruption. Reachable via DecodedSource::from_sound on a hostile .wav (FLAC/OGG paths apply their own rate bounds but WAV has none). Fix: clamp/validate source_rate to a sane range and cap out_frames before allocating.

## sec-12 — crates/lunar-image/src/decode.rs:147 (with format.rs:124-126)

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

The .li decoder passes width*height*4 (from untrusted 32-bit header dimensions, unbounded in Header::parse) as the zstd decompression capacity with no upper bound and no cross-check against either the compressed input size or the chunk header's own uncompressed_size field (parsed but ignored); zstd 0.13 bulk::decompress (experimental feature off, so no decompressBound cap) does Vec::with_capacity(capacity) up front, so a 16-byte header declaring e.g. 40000x40000 forces a ~6.4 GB allocation request. On memory-limited or overcommit-off systems this aborts the process outright; on default Linux an attacker upgrades the transient virtual reservation to real memory exhaustion with an RLE-heavy zstd payload (~32,000:1 expansion, ~200 KB file fills 6.4 GB). Reachable at runtime via lunar-assets LiTextureLoader (lib.rs:759-761, registered for the "li" extension). The *4 also wraps usize for extreme dimensions in release (and much sooner on wasm32), but wrap-to-small yields a decode error, not corruption — DoS classification stands.

## sec-13 — crates/lunar-image/src/format.rs:125 (decode.rs:179)

- **impact:** medium
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

expected_pixel_bytes (format.rs:125) and the mirrored n_pixels (decode.rs:179) compute width*height(*4) with unchecked `*` on attacker-controlled u32 header dimensions, and the decode path has no dimension guard (the DimensionsTooLarge variant is defined but never used; encode.rs:55 shares the same unchecked multiply). Root [profile.release] has panic=abort and no overflow-checks, so the product wraps silently: on wasm32 (32-bit usize, built in CI) 65536×65536 wraps to 0, and on 64-bit 2^31×2^31 wraps the *4 to 0. A hostile .li whose decompressed payload matches the wrapped size passes the `decompressed.len() != expected_bytes` check, yielding an Image whose pixels.len() is far smaller than width*height*4 — breaking the documented invariant. Downstream (LiTextureLoader → AssetServer::update, gen_mips default true) this blows up in Texture::generate_mipmaps (`prev_pixels[(py*prev_w+px)*4]`), in reinterleave_rgba's `split_at`, or in a multi-exabyte Vec allocation. Crucially these are Rust bounds-checked indexes / allocation failures, so the effect is a PANIC, not an unsafe out-of-bounds memory read: under panic=abort it is a denial-of-service (process abort) with amplification — a few-hundred-byte file triggers the blow-up — not memory corruption or UB. Correct classification: low/medium DoS on hostile assets, not an OOB-read/soundness bug. Fix: validate dimensions (checked/u64 arithmetic, reject when width*height*4 doesn't fit usize) in Header::parse or at the top of decode.

## sec-14 — crates/lunar-plugin-loader/src/lib.rs:439,456-467

- **impact:** medium
- **effort:** L
- **verdict:** confirmed (survived refute-by-default verification)

BehaviorDylibLoader::load/reload (crates/lunar-plugin-loader/src/lib.rs:456-484) are safe public fns that pass *mut BehaviorRegistry — a non-repr(C) Rust struct, with each cdylib statically linking its own lunar-core — across the cdylib boundary, making them unsound APIs by Rust convention. This is a known, documented, deliberately test-gated limitation (ABI CAVEAT + TODO at lib.rs:432-438; commit 49a0c4d), the only in-repo caller is an #[ignore]d test whose header notes it currently segfaults even with same-workspace builds, and it is not a hostile-asset vector (dlopen of an untrusted dylib is arbitrary code execution regardless of layout). Residual risk is confined to downstream users of the public API. Remediation: land the already-planned C-ABI registration shim, and until then mark load/reload unsafe (or feature/doc(hidden)-gate them) so the soundness contract is explicit. impact: low, effort: L.

## sec-15 — bindings/c/src/lib.rs:164-165 (RegisteredSystem Send/Sync impls)

- **impact:** low
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

`unsafe impl Send`/`unsafe impl Sync` for `RegisteredSystem` (bindings/c/src/lib.rs:164-165) carry no SAFETY comment despite asserting that a caller-supplied `user_data: *mut c_void` may move to and be shared across threads. The impls are load-bearing (FfiRegistry, a bevy Resource requiring Send+Sync, stores RegisteredSystem in its maps and scratch Vec) and are in practice sound — dispatch_systems takes &mut World and invokes callbacks serially — but that argument appears nowhere at the impl site. The analogous CsBehavior impls at lines 1351-1352 document theirs via the SAFETY comment at 1348-1350, making this the crate's only undocumented Send/Sync assertion.

## sec-16 — bindings/c/src/lib.rs:444 (lunar_component_insert)

- **impact:** low
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

lunar_component_insert wraps the caller-supplied data pointer with NonNull::new_unchecked without a null check — instant UB on a null pointer from C# — which is inconsistent with the function's own defensive posture: it validates the size argument against the registered layout specifically because callers get arguments wrong, yet skips the cheaper null guard on the same call.

## sec-17 — crates/lunar-render-3d/src/passes.rs:982-987

- **impact:** low
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

The GpuParticle-to-bytes cast at passes.rs:982-987 (`slice::from_raw_parts(gpu as *const GpuParticle as *const u8, PARTICLE_STRIDE as usize)`) has no SAFETY comment and no compile-time guard tying PARTICLE_STRIDE (80) to size_of::<GpuParticle>(); it is sound today (repr(C), 20 f32 fields = exactly 80 bytes, no padding), but a future field edit that introduces padding becomes an uninitialized-memory read (UB), one that shrinks the struct below 80 bytes becomes an out-of-bounds read (UB), and one that grows it becomes a silent GPU-layout mismatch. The crate already models the fix twice: VERTEX_STRIDE is derived from size_of::<GpuVertex3d>() (lib.rs:204) and sibling GPU structs derive bytemuck::Pod (lib.rs:194, 341) — deriving Pod/Zeroable on GpuParticle and using bytemuck::bytes_of (or at minimum a const assert on size) removes the unsafe entirely.
