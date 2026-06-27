//! cargo xtask: build/dist/run task runner.
//!
//! usage:
//!   cargo xtask build [--release]            build C# plugin + Rust binary
//!   cargo xtask dist --modular               release build, separate binary + .so → dist/
//!   cargo xtask dist --single                release build, .so embedded in binary → dist/
//!   cargo xtask run                          debug build (CoreCLR), then run platform_demo_cs
//!
//! debug builds use the CoreCLR hosting path (dotnet build → .dll, no NativeAOT).
//! release / dist builds use NativeAOT (dotnet publish -p:PublishAot=true → .so).

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
    let release = rest.iter().any(|a| a == "--release");
    let single  = rest.iter().any(|a| a == "--single");
    let modular = rest.iter().any(|a| a == "--modular");

    let result = match task.as_str() {
        "build" => build(release),
        "dist"  => {
            if single && modular {
                eprintln!("error: --single and --modular are mutually exclusive");
                return ExitCode::FAILURE;
            }
            if single { dist_single() } else { dist_modular() }
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

/// debug build: CoreCLR path. dotnet build → .dll, Rust with coreclr feature.
fn build(release: bool) -> Result<(), String> {
    let root = workspace_root();

    if release {
        build_release(&root)
    } else {
        build_debug(&root)
    }
}

fn build_debug(root: &Path) -> Result<(), String> {
    // 1. LunarHost bootstrapper (always managed, never NativeAOT)
    let host_dir     = root.join("bindings/dotnet-host");
    let host_out_dir = host_dir.join("publish");
    cmd("dotnet", &[
        "publish", "-c", "Debug",
        "-o", host_out_dir.to_str().unwrap(),
    ], &host_dir)?;

    // 2. game plugin as managed .dll (no NativeAOT)
    let plugin_dir  = root.join("examples/platform_demo_cs/plugin");
    let plugin_out  = plugin_dir.join("publish");
    cmd("dotnet", &[
        "publish", "-c", "Debug",
        "-o", plugin_out.to_str().unwrap(),
    ], &plugin_dir)?;

    // 3. Rust binary (coreclr is a default feature, no flag needed)
    cmd("cargo", &["build", "--example", "platform_demo_cs"], root)
}

fn build_release(root: &Path) -> Result<(), String> {
    // 1. game plugin as NativeAOT .so (no bootstrapper needed)
    let plugin_dir  = root.join("examples/platform_demo_cs/plugin");
    let publish_dir = plugin_dir.join("publish");
    cmd("dotnet", &[
        "publish",
        "-r", dotnet_rid(),
        "-c", "Release",
        "-p:PublishAot=true",
        "-o", publish_dir.to_str().unwrap(),
    ], &plugin_dir)?;

    // 2. Rust binary with NativeAOT path (coreclr is a default feature, opt out for release)
    cmd("cargo", &["build", "--release", "--no-default-features", "--example", "platform_demo_cs"], root)
}

/// modular distribution: binary + .so as separate files.
fn dist_modular() -> Result<(), String> {
    let root     = workspace_root();
    let dist_dir = root.join("dist/modular");
    std::fs::create_dir_all(&dist_dir).map_err(|e| e.to_string())?;

    build_release(&root)?;

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

/// single-file distribution: .so bytes embedded in the binary.
fn dist_single() -> Result<(), String> {
    let root     = workspace_root();
    let dist_dir = root.join("dist/single");
    std::fs::create_dir_all(&dist_dir).map_err(|e| e.to_string())?;

    let plugin_dir  = root.join("examples/platform_demo_cs/plugin");
    let publish_dir = plugin_dir.join("publish");
    cmd("dotnet", &[
        "publish",
        "-r", dotnet_rid(),
        "-c", "Release",
        "-p:PublishAot=true",
        "-o", publish_dir.to_str().unwrap(),
    ], &plugin_dir)?;

    let lib_path = publish_dir.join(plugin_lib_name());
    println!("» embedding {} into binary", lib_path.display());
    cmd_env(
        "cargo",
        &["build", "--release", "--no-default-features", "--example", "platform_demo_cs"],
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

/// debug run: build via coreclr path, then launch with host dll + plugin dll paths.
fn run() -> Result<(), String> {
    let root = workspace_root();

    build_debug(&root)?;

    let host_dll   = root.join("bindings/dotnet-host/publish/LunarHost.dll");
    let plugin_dll = root
        .join("examples/platform_demo_cs/plugin/publish")
        .join(plugin_dll_name());

    cmd("cargo", &[
        "run",
        "--example", "platform_demo_cs",
        "--",
        host_dll.to_str().unwrap(),
        plugin_dll.to_str().unwrap(),
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

fn plugin_dll_name() -> &'static str {
    "lunar_scripts.dll"
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
