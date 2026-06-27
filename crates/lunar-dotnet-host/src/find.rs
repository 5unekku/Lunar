//! locates hostfxr.so/dylib/dll at runtime without linking to libnethost.

use std::path::{Path, PathBuf};

pub(crate) fn find_hostfxr() -> Option<PathBuf> {
    // 1. DOTNET_ROOT env var: fastest path, set by installers and containers
    if let Ok(root) = std::env::var("DOTNET_ROOT") {
        if let Some(p) = latest_in(Path::new(&root)) {
            return Some(p);
        }
    }

    // 2. well-known system locations
    for root in system_roots() {
        if let Some(p) = latest_in(&root) {
            return Some(p);
        }
    }

    // 3. ask the dotnet CLI (slowest path, spawns a process)
    if let Some(root) = root_from_cli() {
        if let Some(p) = latest_in(&root) {
            return Some(p);
        }
    }

    None
}

/// find the highest-versioned hostfxr under `<root>/host/fxr/`.
fn latest_in(dotnet_root: &Path) -> Option<PathBuf> {
    let fxr_dir = dotnet_root.join("host/fxr");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&fxr_dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    versions.sort();
    let lib = versions.last()?.join(hostfxr_lib_name());
    lib.exists().then_some(lib)
}

fn hostfxr_lib_name() -> &'static str {
    if cfg!(target_os = "windows") { "hostfxr.dll" }
    else if cfg!(target_os = "macos") { "libhostfxr.dylib" }
    else { "libhostfxr.so" }
}

fn system_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/usr/share/dotnet"),
        PathBuf::from("/usr/local/share/dotnet"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".dotnet"));
    }
    // windows default
    if cfg!(target_os = "windows") {
        roots.push(PathBuf::from(r"C:\Program Files\dotnet"));
    }
    roots
}

fn root_from_cli() -> Option<PathBuf> {
    let output = std::process::Command::new("dotnet")
        .arg("--info")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        // dotnet --info prints: "  Install location:   /usr/share/dotnet"
        if let Some(rest) = line.trim().strip_prefix("Install location:") {
            let p = PathBuf::from(rest.trim());
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}
