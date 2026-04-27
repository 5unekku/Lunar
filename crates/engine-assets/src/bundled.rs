//! bundled assets for WASM targets.
//!
//! this module provides a compile-time asset bundling system using `include_bytes!`.
//! assets are embedded directly into the WASM binary, avoiding the need for network requests.
//!
//! # usage
//!
//! add assets to the bundled assets map in your build script or manually:
//!
//! ```ignore
//! // in build.rs or manually
//! bundled::register("assets/sprite.png", include_bytes!("../../assets/sprite.png").to_vec());
//! ```
//!
//! the asset server will automatically check bundled assets before falling back to fetch.

use std::collections::HashMap;
use std::sync::Mutex;

/// global registry of bundled assets.
static BUNDLED_ASSETS: Mutex<Option<HashMap<String, Vec<u8>>>> = Mutex::new(None);

fn ensure_map() {
    let mut guard = BUNDLED_ASSETS.lock().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
}

/// register a bundled asset at compile time.
///
/// this should be called during initialization before any asset loading.
pub fn register(path: &str, data: Vec<u8>) {
    ensure_map();
    let mut guard = BUNDLED_ASSETS.lock().unwrap();
    guard.as_mut().unwrap().insert(path.to_string(), data);
}

/// register multiple assets from a hashmap.
pub fn register_many(assets: HashMap<String, Vec<u8>>) {
    ensure_map();
    let mut guard = BUNDLED_ASSETS.lock().unwrap();
    let map = guard.as_mut().unwrap();
    for (path, data) in assets {
        map.insert(path, data);
    }
}

/// check if an asset is available in the bundle.
pub fn contains(path: &str) -> bool {
    let guard = BUNDLED_ASSETS.lock().unwrap();
    guard.as_ref().is_some_and(|m| m.contains_key(path))
}

/// get a bundled asset by path.
///
/// returns the raw bytes if the asset was found.
pub fn get(path: &str) -> Option<Vec<u8>> {
    let guard = BUNDLED_ASSETS.lock().unwrap();
    guard.as_ref().and_then(|m| m.get(path).cloned())
}

/// get all registered asset paths.
pub fn paths() -> Vec<String> {
    let guard = BUNDLED_ASSETS.lock().unwrap();
    guard
        .as_ref()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// clear all bundled assets (useful for testing).
pub fn clear() {
    let mut guard = BUNDLED_ASSETS.lock().unwrap();
    *guard = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_get_asset() {
        clear();
        register("test.png", vec![1, 2, 3]);
        assert!(contains("test.png"));
        assert_eq!(get("test.png"), Some(vec![1, 2, 3]));
        assert!(!contains("missing.png"));
        assert_eq!(get("missing.png"), None);
    }

    #[test]
    fn register_many_assets() {
        clear();
        let mut assets = HashMap::new();
        assets.insert("a.png".to_string(), vec![1]);
        assets.insert("b.png".to_string(), vec![2]);
        register_many(assets);
        assert_eq!(paths().len(), 2);
        assert!(contains("a.png"));
        assert!(contains("b.png"));
    }

    #[test]
    fn paths_returns_all() {
        clear();
        register("x.png", vec![]);
        register("y.png", vec![]);
        let mut p = paths();
        p.sort();
        assert_eq!(p, vec!["x.png", "y.png"]);
    }
}
