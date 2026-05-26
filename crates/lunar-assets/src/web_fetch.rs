//! web asset loading via the fetch API
//!
//! provides [`fetch_bytes`] for downloading raw asset data over HTTP on WASM targets.
//! the [`IoTaskPool`](super::IoTaskPool) calls this directly; use it for any ad-hoc
//! network fetch from game code as well.

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{RequestInit, RequestMode, Response};

/// fetch raw bytes from a URL using the browser's fetch API.
///
/// returns the full response body on success, or an error string on failure.
/// uses CORS mode so cross-origin assets work when the server sends the right headers.
pub async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("no window available")?;

    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::Cors);

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

    Ok(js_sys::Uint8Array::new(&array_buffer).to_vec())
}
