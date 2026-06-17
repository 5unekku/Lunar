//! cargo xtask — build/dist/run task runner.
//!
//! usage:
//!   cargo xtask build [--release]            build C# plugin + Rust binary
//!   cargo xtask dist --modular               release build, separate binary + .so → dist/
//!   cargo xtask dist --single                release build, .so embedded in binary → dist/
//!   cargo xtask run                          debug build, then run platform_demo_cs

use std::{
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let task = match args.next() {
        Some(t) => t,
        None => {
            eprintln!("usage: cargo xtask <build|dist|run> [--release] [--single|--modular]");
            return ExitCode::FAILURE;
        }
    };
    let rest: Vec<String> = args.collect();
    let release  = rest.iter().any(|a| a == "--release");
    let single   = rest.iter().any(|a| a == "--single");
    let modular  = rest.iter().any(|a| a == "--modular");

    let result = match task.as_str() {
        "build" => build(release),
        "dist"  => {
            if single && modular {
                eprintln!("error: --single and --modular are mutually exclusive");
                return ExitCode::FAILURE;
            }
            if single  { dist_single() }
            else       { dist_modular() }  // --modular is the default
        }
        "run"   => run(),
        other   => {
            eprintln!("unknown task '{other}'. available: build, dist, run");
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => { eprintln!("xtask failed: {e}"); ExitCode::FAILURE }
    }
}

// ── tasks ─────────────────────────────────────────────────────────────────────

fn build(release: bool) -> Result<(), String> {
    let root = workspace_root();
    let profile = if release { "Release" } else { "Debug" };

    // 1. C# plugin
    let plugin_dir  = root.join("examples/platform_demo_cs/plugin");
    let publish_dir = plugin_dir.join("publish");
    cmd("dotnet", &[
        "publish",
        "-r", dotnet_rid(),
        "-c", profile,
        "-o", publish_dir.to_str().unwrap(),
    ], &plugin_dir)?;

    // 2. Rust example
    let mut rust_args = vec!["build", "--example", "platform_demo_cs"];
    if release { rust_args.push("--release"); }
    cmd("cargo", &rust_args, &root)
}

/// modular profile: binary + .so as separate files.
/// smallest patch unit: a renderer fix ships as a new binary, a script fix as a new .so.
fn dist_modular() -> Result<(), String> {
    let root     = workspace_root();
    let dist_dir = root.join("dist/modular");
    std::fs::create_dir_all(&dist_dir).map_err(|e| e.to_string())?;

    build(true)?;

    let bin_name = binary_name();
    let lib_name = plugin_lib_name();
    copy(
        &root.join("target/release/examples").join(bin_name),
        &dist_dir.join(bin_name))?;
    copy(
        &root.join("examples/platform_demo_cs/plugin/publish").join(lib_name),
        &dist_dir.join(lib_name))?;

    println!("dist/modular/  →  {bin_name}  +  {lib_name}");
    Ok(())
}

/// single profile: .so bytes embedded in the binary — one file to ship.
fn dist_single() -> Result<(), String> {
    let root     = workspace_root();
    let dist_dir = root.join("dist/single");
    std::fs::create_dir_all(&dist_dir).map_err(|e| e.to_string())?;

    // build C# first so we can embed the .so
    let plugin_dir  = root.join("examples/platform_demo_cs/plugin");
    let publish_dir = plugin_dir.join("publish");
    cmd("dotnet", &[
        "publish",
        "-r", dotnet_rid(),
        "-c", "Release",
        "-o", publish_dir.to_str().unwrap(),
    ], &plugin_dir)?;

    let lib_path = publish_dir.join(plugin_lib_name());

    // build Rust with the plugin path set so build.rs embeds it
    println!("» embedding {} into binary", lib_path.display());
    cmd_env(
        "cargo",
        &["build", "--release", "--example", "platform_demo_cs"],
        &root,
        &[("LUNAR_CS_PLUGIN_PATH", lib_path.to_str().unwrap())],
    )?;

    let bin_name = binary_name();
    copy(
        &root.join("target/release/examples").join(bin_name),
        &dist_dir.join(bin_name))?;

    println!("dist/single/  →  {bin_name}  (plugin embedded)");
    Ok(())
}

fn run() -> Result<(), String> {
    let root       = workspace_root();
    let plugin_lib = root
        .join("examples/platform_demo_cs/plugin/publish")
        .join(plugin_lib_name());

    build(false)?;

    cmd("cargo", &[
        "run",
        "--example", "platform_demo_cs",
        "--",
        plugin_lib.to_str().unwrap(),
    ], &root)
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask has no parent directory")
        .to_owned()
}

fn dotnet_rid() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux",   "x86_64")  => "linux-x64",
        ("linux",   "aarch64") => "linux-arm64",
        ("macos",   "x86_64")  => "osx-x64",
        ("macos",   "aarch64") => "osx-arm64",
        ("windows", "x86_64")  => "win-x64",
        ("windows", "aarch64") => "win-arm64",
        (os, arch) => panic!("unsupported platform: {os}/{arch}"),
    }
}

fn binary_name() -> &'static str {
    if cfg!(windows) { "platform_demo_cs.exe" } else { "platform_demo_cs" }
}

fn plugin_lib_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "lunar_scripts.dll",
        "macos"   => "lunar_scripts.dylib",
        _         => "lunar_scripts.so",
    }
}

fn cmd(program: &str, args: &[&str], dir: &Path) -> Result<(), String> {
    cmd_env(program, args, dir, &[])
}

fn cmd_env(program: &str, args: &[&str], dir: &Path, env: &[(&str, &str)]) -> Result<(), String> {
    println!("» {program} {}", args.join(" "));
    let mut command = Command::new(program);
    command.args(args).current_dir(dir);
    for (key, value) in env {
        command.env(key, value);
    }
    let status = command.status()
        .map_err(|e| format!("failed to spawn '{program}': {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("'{program}' exited with {}", status.code().unwrap_or(1)))
    }
}

fn copy(src: &Path, dst: &Path) -> Result<(), String> {
    println!("» cp {} → {}", src.display(), dst.display());
    std::fs::copy(src, dst)
        .map(|_| ())
        .map_err(|e| format!("copy failed: {e}"))
}
