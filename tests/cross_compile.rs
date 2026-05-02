//! cross-compile compatibility checks.
//!
//! runs `cargo check --target <triple>` for each supported platform to catch
//! platform-specific compile errors early.
//!
//! prerequisites: run `rustup target add <triple>` for each target you want to check.
//! targets without the rustup target installed are skipped with a warning.

const TARGETS: &[(&str, &str)] = &[
    ("x86_64-unknown-linux-gnu", "linux"),
    ("x86_64-apple-darwin", "macos"),
    ("x86_64-pc-windows-msvc", "windows"),
    ("wasm32-unknown-unknown", "web"),
];

fn target_installed(target: &str) -> bool {
    let output = std::process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .expect("failed to run rustup");

    if !output.status.success() {
        return false;
    }

    let installed = String::from_utf8_lossy(&output.stdout);
    installed.lines().any(|line| line == target)
}

fn check_target(target: &str, label: &str) {
    if !target_installed(target) {
        println!("skipping {label} ({target}) — run: rustup target add {target}");
        return;
    }

    println!("checking {label} ({target})...");

    let output = std::process::Command::new("cargo")
        .args([
            "check",
            "--target",
            target,
            "--workspace",
            "--exclude",
            "lunar-game",
        ])
        .output()
        .expect("failed to run cargo check");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("cross-compile check failed for {label} ({target}):\n{stderr}");
    }

    println!("  {label} ({target}) ok");
}

#[test]
fn cross_compile_linux() {
    let (target, label) = TARGETS[0];
    check_target(target, label);
}

#[test]
fn cross_compile_macos() {
    let (target, label) = TARGETS[1];
    check_target(target, label);
}

#[test]
fn cross_compile_windows() {
    let (target, label) = TARGETS[2];
    check_target(target, label);
}

#[test]
fn cross_compile_web() {
    let (target, label) = TARGETS[3];
    check_target(target, label);
}
