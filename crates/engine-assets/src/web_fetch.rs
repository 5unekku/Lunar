//! web asset loading via the fetch API
//!
//! provides [`fetch_bytes`] for downloading assets over HTTP on WASM targets.
//! on native targets this module is a stub that uses std::fs::read.

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{RequestInit, RequestMode, Response};

/// fetch raw bytes from a URL using the browser's fetch API.
/// returns the bytes on success or an error string on failure.
pub async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("no window available")?;

    let mut opts = RequestInit::new();
    opts.method("GET");
    opts.mode(RequestMode::Cors);

    let request = web_sys::Request::new_with_str_and_init(url, &opts)
        .map_err(|e| format!("failed to create request: {e:?}"))?;

    let resp_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("fetch failed: {e:?}"))?;

    let resp: Response = resp_value
        .dyn_into()
        .map_err(|_| "response is not a Response object")?;

    if !resp.ok() {
        return Err(format!("HTTP error: status {}", resp.status()));
    }

    let array_buffer = JsFuture::from(
        resp.array_buffer()
            .map_err(|e| format!("array_buffer failed: {e:?}"))?,
    )
    .await
    .map_err(|e| format!("failed to read body: {e:?}"))?;

    let bytes = js_sys::Uint8Array::new(&array_buffer).to_vec();
    Ok(bytes)
}

/// fetch a texture from a URL and return the raw bytes.
pub async fn fetch_texture(url: &str) -> Result<Vec<u8>, String> {
    fetch_bytes(url).await
}

/// fetch a sound from a URL and return the raw bytes.
pub async fn fetch_sound(url: &str) -> Result<Vec<u8>, String> {
    fetch_bytes(url).await
}

/// fetch a font from a URL and return the raw bytes.
pub async fn fetch_font(url: &str) -> Result<Vec<u8>, String> {
    fetch_bytes(url).await
}
