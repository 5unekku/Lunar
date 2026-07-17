# build & platform matrix audit

date: 2026-07-16
concern: build and platform matrix

method: multi-agent fan-out (one finder agent per lens), location dedup, then every
surviving finding was handed to an independent adversarial verifier whose default was
to refute it — only findings that held up under the verifier's own reading of the code
are listed below. read-only audit; no source was modified. this doc was transcribed by
the orchestrator from the verified finding set after the workflow's writer stage was
cut off by a subagent model-credit limit — the findings and verdicts are the agents', unedited.

scope: the ci pipeline, build-script/xtask duplication, cross-target hygiene, and toolchain/packaging.

**12 confirmed findings** (1 critical, 2 high, 8 medium, 1 low).

discovery stats: 52 raw findings from 4 lenses (+0 gap follow-ups), 50 after dedup, 12 confirmed, 0 refuted.

every finding carries: id, location (file:line), impact, effort (S/M/L), and the
verified claim (verifier-corrected wording where the skeptic adjusted it). ids are
assigned in severity order and are stable references for the phase-2 backlog synthesis.

---

## build-01 — CONTRIBUTING.md:35-38 (CI home) / .github/workflows/ci.yml:1

- **impact:** critical
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

The CI gate described in CONTRIBUTING does not exist in practice: every ci.yml run in the repo's history failed (8/8), the last run was 2026-06-13, and 135 commits (2026-06-12 to 2026-07-16, including the entire render-3d perf program) have never been CI-validated because the GitHub mirror stopped receiving pushes and the primary GitLab remote has no .gitlab-ci.yml at all.

## build-02 — .github/workflows/ci.yml:13-93 (job list; absence of native mac/windows legs)

- **impact:** high
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

Despite 'mac must keep working' and 'native windows first-class' platform goals, no CI leg ever compiles on a mac host or with the msvc toolchain, and nothing windows ever executes: the matrix covers only *-windows-gnu/gnullvm via zig cross-compile (ci.yml:72-78), so darwin cfg paths (.cargo/config.toml macos link-args, xtask osx RIDs) and windows-msvc linkage can rot invisibly.

## build-03 — .github/workflows/ci.yml:86-93 (build-cross aarch64 full leg)

- **impact:** high
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

The aarch64-unknown-linux-gnu full-workspace cross leg failed in the last recorded CI run (job 81149786718, 2026-06-13) because cargo-zigbuild 0.22.3's zig wrapper passes through rustc's new aarch64 target-spec link arg --fix-cortex-a53-843419, which zig rejects. Nothing in ci.yml since addresses it: line 86 still installs cargo-zigbuild unpinned via pip, so whether the leg passes today depends on runtime pip resolution (plus the floating rustc nightly and ziglang wheel). Upstream fixed this in cargo-zigbuild v0.23.0 (2026-06-18, PR #452), released after the failed run — fix (S): pin `pip install cargo-zigbuild>=0.23.0` (or exact-pin 0.23.0) at ci.yml:86 and confirm on a real run.

## build-04 — .github/workflows/ (only ci.yml and docs.yml exist; no dependency-audit leg)

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

No cargo-deny/cargo-audit leg exists (no deny.toml, no dependabot either), so RUSTSEC advisories, duplicate-major dependency creep, and license drift in a 116 KB Cargo.lock (including vendored-C sdl3-sys, libloading, wgpu) are unchecked for a mixed-license workspace (MPL-2.0 core with Section 7 additional permission; lunar/lunar-math/lunar-macros under MIT OR Apache-2.0; hand-maintained 3-entry third-party NOTICE) that plans a crates.io release job. Duplicate-major creep is already present, not hypothetical: the lock carries glam 0.29.3/0.30.10/0.33.2 and hashbrown 0.15/0.16/0.17 among others, so a `bans` multiple-versions check would fire on day one and the initial wiring needs a triage pass, not just the one-step action sketch.

## build-05 — .github/workflows/ci.yml (no bench leg) + xtask/src/main.rs:31-44

- **impact:** medium
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

For the planned phase-0 render-bench leg (tools/render-bench + `cargo xtask bench`), the right CI placement is a separate scheduled/manually-dispatched workflow off the PR critical path: CI's only Vulkan adapter is lavapipe (installed in the test and smoke jobs; all jobs run on shared ubuntu-latest runners), so bench numbers there are software-rasterizer timings with shared-runner variance — useful as a does-it-run regression smoke and artifact trail, never as a perf gate. Run it report-only first and add thresholds only after variance is measured; real perf numbers remain a pinned-hardware concern per the improvement program's baseline methodology (RADV dev-machine baselines, hostname+adapter-keyed results). The spec's CPU-only criterion micro-benches are a separate question and may run in ordinary CI containers as already planned.

## build-06 — .github/workflows/ci.yml (no fuzz leg) + crates/lunar-image/src/decode.rs:113

- **impact:** medium
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

No fuzz leg exists in CI, and the workspace's one genuinely hand-rolled untrusted-bytes parser — `lunar_image::decode` (decode.rs:113) — mixes checked and unchecked size arithmetic on attacker-controlled header fields: `Header::expected_pixel_bytes()` (format.rs:125) computes `(width as usize)*(height as usize)*4` unchecked (overflowable on the CI matrix's own 32-bit i686/armv7 targets), and that value plus an unchecked filter-overhead add (decode.rs:143) becomes the allocation capacity passed to `zstd::bulk::decompress` (decode.rs:147), so a tiny crafted .li file can force a multi-GB allocation; with release panic=abort any parser panic is a hard game crash. A cargo-fuzz target for image decode is cheap on the pinned nightly toolchain (scheduled workflow, off the PR path, corpus cached). The BSP loader is bincode-over-serde rather than hand-rolled, and its actual gap is missing post-deserialize validation — `camera_leaf` (level.rs:120-136) trusts blob-supplied node indices (out-of-bounds panic) and can infinite-loop on cyclic blobs — so a BSP fuzz target is only worthwhile if the harness drives the accessors (or, cheaper, add structural validation in from_binary). The gamedata loader is a trusted build-time blob embedded via include_bytes! and is not a meaningful fuzz target.

## build-07 — .github/workflows/ci.yml:21 + xtask/src/main.rs:102

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

The shipping NativeAOT configuration is never compiled by CI: xtask release builds use `--no-default-features` (coreclr off), but every CI leg builds default features only, so the non-coreclr code path in lunar-plugin-loader/lunar-game can break without any signal.

## build-08 — .github/workflows/ci.yml:3-5 (trigger block; no concurrency group)

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

ci.yml:3-5's bare `on: push / pull_request` with no `concurrency` group is a latent double-run misconfiguration: any same-repo branch with an open PR would run the 11-job, ~90-compute-minute pipeline twice per push, with superseded runs never cancelled. To date this is theoretical — the repo has zero PRs and all 16 historical runs are direct pushes to master — and the repo is public, so the cost is not billed minutes but the 20-concurrent-job free-tier cap (22 queued jobs per duplicated push) and slower feedback; exposure grows when the planned bench CI leg lands or outside contributions start. Fix (S): `on: { push: { branches: [master] }, pull_request: }` plus `concurrency: { group: ci-${{ github.ref }}, cancel-in-progress: ${{ github.ref != 'refs/heads/master' }} }`, noting two consequences: branch pushes without an open PR would no longer get any CI (today they do), and back-to-back master pushes still run to completion unconcurrently-cancelled by design.

## build-09 — .github/workflows/ci.yml:61-64 (ENGINE_CRATES) vs Cargo.toml workspace members

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

The C ABI surface (lunar-ffi, bindings/c) has no green cross-target build coverage outside x86_64-linux — a real gap for pointer-width-sensitive FFI, sharpened by the aarch64 full leg being red — but NOT because ENGINE_CRATES mis-implements its comment: lunar-ffi is transitively sdl3-dependent (lunar-ffi -> lunar-input -> sdl3 on every non-wasm target, used at bindings/c/src/lib.rs:40), and lunar-plugin-loader inherits sdl3 via lunar-ffi, so both are correctly excluded under the documented sdl3 policy. Adding -p lunar-ffi to ENGINE_CRATES would break all five non-full legs (sdl3 0.18.4 XlibWindowHandle u64-vs-u32 compile error on i686/armv7; MinGW windres on windows targets). Closing the gap requires decoupling lunar-ffi's input surface from sdl3-backed lunar-input (e.g. sdl3-free types split or feature gate) — effort M, a designed change — plus an S comment tweak clarifying that 'sdl3-dependent' includes transitive dependents. Separately, the aarch64 full-leg red is an unrelated zigbuild/lld failure ('unsupported linker arg: --fix-cortex-a53-843419') at the lunar-game bin link; lunar-ffi itself compiled for aarch64 in that run.

## build-10 — .github/workflows/ci.yml:86 + ci.yml:7-11 (no permissions block)

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

ci.yml:86 installs cargo-zigbuild unpinned from PyPI (`pip install cargo-zigbuild --break-system-packages` — no version, no hash), and the PyPI package additionally pulls an unpinned `ziglang` wheel, so both cargo-zigbuild and the zig toolchain backing all 7 cross triples drift silently between CI runs — a reproducibility and supply-chain gap (no observed CI breakage is attributable to this yet). Separately, ci.yml lacks a top-level `permissions:` block (docs.yml has one); the repo's default workflow token is already read-only, so this is defense-in-depth hardening rather than an active over-grant, but an explicit `permissions: { contents: read }` makes the intent survive any future settings change. Fix (S): add the permissions block and pin cargo-zigbuild (e.g. `cargo-zigbuild==0.20.*` plus a pinned ziglang, or install the GitHub release binary by checksum).

## build-11 — rust-toolchain.toml:2 + .github/workflows/ci.yml:16

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

Floating `channel = "nightly"` (rust-toolchain.toml:2) plus the mutable `rust:slim-trixie` container tag (ci.yml:16/25/36/46/56) makes CI non-reproducible and defeats cross-day caching: Swatinem/rust-cache hashes the exact rustc version into both its exact and restore keys, so every nightly bump invalidates all 11 job caches (verified in run 27452249632: 'No cache found.' on a repeat run, registry re-downloads), and since rust-cache's cache paths exclude $RUSTUP_HOME, every job of every run re-downloads nightly + cranelift-preview + wasm32 std into a container that ships only stable (1.96.0). Caching still works for same-day repeat pushes; everything else is a guaranteed miss.

## build-12 — .github/workflows/ci.yml:13-52 (no fmt step) + .hooks/pre-commit:18-23

- **impact:** low
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

Formatting is enforced nowhere: CI (.github/workflows/ci.yml, jobs at lines 13-93) has no `cargo fmt --check` step, and the repo's .hooks/pre-commit that would run it (lines 18-23) is not wired up (core.hooksPath unset at all scopes, .git/hooks/ has only README.sample, no CONTRIBUTING/README/xtask wiring), so the rustfmt.toml style (hard_tabs = true; rustfmt component pinned in rust-toolchain.toml) is unenforced — and demonstrably so: the committed tree currently fails `cargo fmt --check` with 354 diff hunks across 47 files. The fix is therefore not a drop-in CI job: it needs (a) a one-time mechanical `cargo fmt` sweep commit (~47 files, review-noise only), then (b) the small fmt CI job (rust:slim-trixie container, checkout, `cargo fmt --check`; no apt deps or cache since fmt does not build the tree — though the pinned nightly toolchain download applies), and optionally (c) a documented `git config core.hooksPath .hooks` line in CONTRIBUTING.md. Note the standalone tools (tools/bake-pvs, tools/compress-textures) are outside the workspace and need their own fmt invocations if they are to be covered.
