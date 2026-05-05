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
