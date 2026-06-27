# Lunar perf + footprint audit

## environment

- rustc: 1.98.0-nightly (cb46fbb8c 2026-06-08)
- host triple: x86_64-unknown-linux-gnu
- date: 2026-06-27

## A. performance gaps

## B. binary size / bloat

### tooling

- cargo-bloat: 0.12.1
- cargo-tree: bundled with cargo (nightly 1.98)
- wasm-opt (binaryen): version 130
- wasm-bindgen: 0.2.123
- gzip: present
- brotli: 1.2.0
- cargo-zigbuild: installed (no --version flag)
- dotnet SDK: 10.0.301

## C. low-risk wins applied

## stretch-goal recommendations
```
