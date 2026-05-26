//! internal proc-macro crate for lunar.
//!
//! Wraps the ECS derives (`Component`, `Resource`, `Event`, `Message`) so they
//! emit paths through `::lunar::__bevy_ecs` instead of `::bevy_ecs`. This is
//! the mechanism that lets game crates depend on `lunar` alone without
//! needing `bevy_ecs` in their `Cargo.toml`.
//!
//! Game code should never name this crate directly — `lunar` re-exports the
//! derives at its crate root (`lunar::Component`, `lunar::Resource`, etc.)
//! and through `lunar::prelude`.
//!
//! # scope
//!
//! These derives cover the minimal trait shape: storage type, mutability,
//! marker traits. Game code that needs `#[component(on_add = …)]` hooks,
//! relationship attributes, required components, or `#[entities]` field mapping
//! must reach for the upstream bevy_ecs derive via the escape hatch
//! `lunar::__bevy_ecs::prelude::Component`. 99% of 2D game code never needs
//! that.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

// ── texture! ──────────────────────────────────────────────────────────────────

/// embed a texture at compile time, converting the source image to `.mi` on demand.
///
/// resolves webp/png/jpg/bmp files from the `assets/` directory, validates that
/// the file's actual format matches its extension (or detects format for bare names),
/// converts to `.mi` (zstd-compressed RGBA) if the cache is stale, and expands to
/// `include_bytes!` so the compiler folds the data directly into the binary.
///
/// # path resolution
///
/// - `texture!("sprites/player")` — finds `player.webp`, `player.png`, etc.
///   fails to compile if none or more than one match exists.
/// - `texture!("sprites/player.webp")` — explicit extension; validates magic bytes.
///
/// # example
///
/// ```ignore
/// let tex = assets.load_texture(texture!("sprites/player"));
/// ```
#[proc_macro]
pub fn texture(input: TokenStream) -> TokenStream {
    match texture_impl(input.into()) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn texture_impl(input: proc_macro2::TokenStream) -> syn::Result<proc_macro2::TokenStream> {
    use std::path::PathBuf;

    let path_lit: syn::LitStr = syn::parse2(input)?;
    let path_str = path_lit.value();
    let span = path_lit.span();

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| syn::Error::new(span, "CARGO_MANIFEST_DIR not set"))?;

    let assets_dir = PathBuf::from(&manifest_dir).join("assets");
    let cache_dir = PathBuf::from(&manifest_dir).join(".lunar");

    let (source_path, format) = resolve_texture(&path_str, &assets_dir, span)?;

    // strip assets/ prefix to get the relative path for the cache
    let relative = source_path.strip_prefix(&assets_dir).unwrap();
    let cache_path = cache_dir.join(relative).with_extension("mi");

    let needs_convert = !cache_path.exists()
        || source_path
            .metadata()
            .and_then(|m| m.modified())
            .and_then(|src| cache_path.metadata().and_then(|m| m.modified()).map(|c| src > c))
            .unwrap_or(true);

    if needs_convert {
        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| syn::Error::new(span, format!("failed to create cache dir: {e}")))?;
        }
        convert_to_mi(&source_path, &cache_path, format, span)?;
    }

    let cache_str = cache_path.to_string_lossy().into_owned();
    Ok(quote! { include_bytes!(#cache_str) })
}

#[derive(Clone, Copy, PartialEq)]
enum ImageFormat { Png, Jpeg, Bmp, WebP }

fn detect_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(b"\xFF\xD8\xFF") {
        Some(ImageFormat::Jpeg)
    } else if bytes.starts_with(b"BM") {
        Some(ImageFormat::Bmp)
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::WebP)
    } else {
        None
    }
}

fn format_from_ext(ext: &str) -> Option<ImageFormat> {
    match ext {
        "png" => Some(ImageFormat::Png),
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        "bmp" => Some(ImageFormat::Bmp),
        "webp" => Some(ImageFormat::WebP),
        _ => None,
    }
}

/// resolve a texture path: find the source file, validate format, return (path, format).
fn resolve_texture(
    path_str: &str,
    assets_dir: &std::path::Path,
    span: proc_macro2::Span,
) -> syn::Result<(std::path::PathBuf, ImageFormat)> {
    use std::path::PathBuf;

    let input_path = PathBuf::from(path_str);
    let ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // explicit extension — one candidate
    if format_from_ext(&ext).is_some() {
        let full = assets_dir.join(path_str);
        if !full.exists() {
            return Err(syn::Error::new(span, format!("asset not found: assets/{path_str}")));
        }
        let bytes = std::fs::read(&full)
            .map_err(|e| syn::Error::new(span, format!("could not read {path_str}: {e}")))?;
        let detected = detect_format(&bytes)
            .ok_or_else(|| syn::Error::new(span, format!("{path_str}: unrecognized image format")))?;
        let expected = format_from_ext(&ext).unwrap();
        if detected != expected {
            return Err(syn::Error::new(
                span,
                format!("{path_str}: extension says {ext} but magic bytes say otherwise — rename the file to match its actual format"),
            ));
        }
        return Ok((full, detected));
    }

    // bare name — search for candidates
    let stem = path_str.trim_end_matches('/');
    let candidates: Vec<(PathBuf, ImageFormat)> = ["webp", "png", "jpg", "jpeg", "bmp"]
        .iter()
        .map(|ext| assets_dir.join(format!("{stem}.{ext}")))
        .filter(|p| p.exists())
        .filter_map(|p| {
            let bytes = std::fs::read(&p).ok()?;
            let fmt = detect_format(&bytes)?;
            Some((p, fmt))
        })
        .chain({
            // also check a bare file with no extension
            let bare = assets_dir.join(stem);
            if bare.exists() {
                let bytes = std::fs::read(&bare).ok();
                bytes
                    .as_deref()
                    .and_then(detect_format)
                    .map(|fmt| vec![(bare, fmt)])
                    .unwrap_or_default()
            } else {
                vec![]
            }
        })
        .collect();

    match candidates.len() {
        0 => Err(syn::Error::new(span, format!("no image found for \"{path_str}\" in assets/ (tried .webp, .png, .jpg, .bmp)"))),
        1 => Ok(candidates.into_iter().next().unwrap()),
        _ => {
            let names: Vec<_> = candidates
                .iter()
                .filter_map(|(p, _)| p.file_name()?.to_str().map(str::to_owned))
                .collect();
            Err(syn::Error::new(
                span,
                format!("ambiguous: multiple images match \"{path_str}\" — use the full name with extension ({})", names.join(", ")),
            ))
        }
    }
}

fn convert_to_mi(
    source: &std::path::Path,
    dest: &std::path::Path,
    _format: ImageFormat,
    span: proc_macro2::Span,
) -> syn::Result<()> {
    let bytes = std::fs::read(source)
        .map_err(|e| syn::Error::new(span, format!("failed to read source: {e}")))?;
    let img = image::load_from_memory(&bytes)
        .map_err(|e| syn::Error::new(span, format!("failed to decode image: {e}")))?;
    let rgba = img.to_rgba8();
    let mi = engine_image::encode(rgba.width(), rgba.height(), &rgba)
        .map_err(|e| syn::Error::new(span, format!("failed to encode .mi: {e}")))?;
    std::fs::write(dest, mi)
        .map_err(|e| syn::Error::new(span, format!("failed to write cache: {e}")))?;
    Ok(())
}

/// Derive `Component` for a type. Generates an `impl` of bevy_ecs's
/// `Component` trait with `StorageType::Table` and `Mutability::Mutable`
/// (the bevy defaults), routed through `lunar::__bevy_ecs`.
#[proc_macro_derive(Component)]
pub fn derive_component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    quote! {
        impl #impl_generics ::lunar::__bevy_ecs::component::Component
            for #name #type_generics #where_clause
        {
            const STORAGE_TYPE: ::lunar::__bevy_ecs::component::StorageType =
                ::lunar::__bevy_ecs::component::StorageType::Table;
            type Mutability = ::lunar::__bevy_ecs::component::Mutable;
        }
    }
    .into()
}

/// Derive `Resource` for a type. `Resource` is a marker trait — no associated
/// items.
#[proc_macro_derive(Resource)]
pub fn derive_resource(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    quote! {
        impl #impl_generics ::lunar::__bevy_ecs::resource::Resource
            for #name #type_generics #where_clause
        {
        }
    }
    .into()
}

/// Derive `Event` for a type. Defaults `Trigger` to `GlobalTrigger`, matching
/// bevy_ecs's own derive default.
#[proc_macro_derive(Event)]
pub fn derive_event(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    quote! {
        impl #impl_generics ::lunar::__bevy_ecs::event::Event
            for #name #type_generics #where_clause
        {
            type Trigger<'__lunar_a> = ::lunar::__bevy_ecs::event::GlobalTrigger;
        }
    }
    .into()
}

/// Derive `Message` for a type. `Message` is a marker trait.
#[proc_macro_derive(Message)]
pub fn derive_message(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    quote! {
        impl #impl_generics ::lunar::__bevy_ecs::message::Message
            for #name #type_generics #where_clause
        {
        }
    }
    .into()
}
