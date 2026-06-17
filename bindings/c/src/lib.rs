//! C ABI bridge for lunar-engine.
//!
//! exposes entity, component, query, system, and time APIs through a stable
//! `extern "C"` surface. all host language bindings (C++, C#, etc.) go through
//! this crate.
//!
//! # integration
//!
//! the engine binary (or test harness) must call [`init_registry`] once after
//! the world is set up, then call [`dispatch_systems`] for each schedule each
//! frame. plugins are loaded separately via `lunar-plugin-loader`.
//!
//! # safety model
//!
//! every `extern "C"` function here receives a `*mut LunarWorld` that is an
//! alias for the engine's `bevy_ecs::world::World`. pointers to component data
//! returned by `lunar_component_get[_mut]` are only valid until the system
//! callback that received the world pointer returns.

use std::{
    alloc::Layout,
    collections::HashMap,
    ffi::{CStr, c_void, c_char},
    ptr::{NonNull, null, null_mut},
    sync::atomic::{AtomicU32, Ordering},
};

use bevy_ecs::{
    component::{ComponentCloneBehavior, ComponentDescriptor, ComponentId, StorageType},
    prelude::*,
    ptr::OwningPtr,
    world::EntityRef,
};
use glam;
use lunar_3d::{
    Camera3d, Camera3dBundle, LocalTransform3d, Material3d, MaterialData, Mesh3d, Mesh3dBundle,
    MeshData, MeshRegistry, Projection, ShadingModel, WorldTransform3d, primitives,
};
use lunar_assets::{Asset, Handle};
use lunar_core::WindowSettings;
use lunar_input::{GamepadAxis, InputState, KeyCode};
use lunar_math::{Color, LocalTransform, WorldTransform};
use lunar_render_3d::{QualitySettings, Sky};

// ─── opaque world handle ─────────────────────────────────────────────────────

/// opaque handle to the engine world — only valid during a system callback.
///
/// the engine passes a pointer to its internal [`bevy_ecs::world::World`].
/// never construct or dereference this type directly.
pub enum LunarWorld {}

#[inline(always)]
unsafe fn world_from_ffi<'a>(world: *mut LunarWorld) -> &'a mut World {
    unsafe { &mut *(world as *mut World) }
}

// ─── C integer handle types ───────────────────────────────────────────────────

/// entity identifier — index into the world's entity table.
pub type LunarEntity = u32;
/// component type identifier — stable for the lifetime of the world.
pub type LunarComponentId = u32;
/// registered system identifier — used to unregister systems.
pub type LunarSystemId = u32;

pub const LUNAR_INVALID_COMPONENT_ID: LunarComponentId = u32::MAX;
pub const LUNAR_INVALID_SYSTEM_ID: LunarSystemId = u32::MAX;
/// sentinel meaning "no camera set" for [`lunar_get_main_camera`].
pub const LUNAR_NULL_ENTITY: LunarEntity = u32::MAX;

// global main-camera entity index (set from Rust via [`set_main_camera_entity`])
static MAIN_CAMERA_ENTITY: AtomicU32 = AtomicU32::new(u32::MAX);

/// set the main camera entity that C# can retrieve with `lunar_get_main_camera`.
/// call this after spawning the camera entity (the index is available immediately even
/// though the entity is spawned deferred via Commands).
pub fn set_main_camera_entity(index: LunarEntity) {
    MAIN_CAMERA_ENTITY.store(index, Ordering::Relaxed);
}

// ─── C value types ────────────────────────────────────────────────────────────

#[repr(C)]
pub struct LunarVec2 { pub x: f32, pub y: f32 }

#[repr(C)]
pub struct LunarVec3 { pub x: f32, pub y: f32, pub z: f32 }

/// quaternion in portable C layout (4-byte aligned, 16 bytes).
///
/// the engine's internal `glam::Quat` may have 16-byte alignment on SIMD builds.
/// the typed accessors (`lunar_get_transform3d` etc.) handle the conversion —
/// do not use raw component access for `LocalTransform3d`.
#[repr(C)]
pub struct LunarQuat { pub x: f32, pub y: f32, pub z: f32, pub w: f32 }

/// 3D transform in portable C layout: 40 bytes, 4-byte aligned.
#[repr(C)]
pub struct LunarTransform3d {
    pub translation: LunarVec3,
    pub rotation:    LunarQuat,
    pub scale:       LunarVec3,
}

/// 2D transform in portable C layout: 20 bytes, 4-byte aligned.
/// matches the engine's [`LocalTransform`] layout exactly.
#[repr(C)]
pub struct LunarTransform2d {
    pub translation: LunarVec2,
    pub rotation:    f32,
    pub scale:       LunarVec2,
}

// ─── schedule constants (exposed as C uint32 enum) ────────────────────────────

pub const LUNAR_SCHEDULE_STARTUP:      u32 = 0;
pub const LUNAR_SCHEDULE_UPDATE:       u32 = 1;
pub const LUNAR_SCHEDULE_FIXED_UPDATE: u32 = 2;
pub const LUNAR_SCHEDULE_SHUTDOWN:     u32 = 3;

/// internal schedule discriminant — convert from raw u32 at the FFI boundary.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LunarSchedule { Startup, Update, FixedUpdate, Shutdown }

impl LunarSchedule {
    fn from_u32(value: u32) -> Option<Self> {
        match value {
            LUNAR_SCHEDULE_STARTUP      => Some(Self::Startup),
            LUNAR_SCHEDULE_UPDATE       => Some(Self::Update),
            LUNAR_SCHEDULE_FIXED_UPDATE => Some(Self::FixedUpdate),
            LUNAR_SCHEDULE_SHUTDOWN     => Some(Self::Shutdown),
            _ => None,
        }
    }
}

// ─── C function pointer types ─────────────────────────────────────────────────

pub type LunarSystemFn = unsafe extern "C" fn(*mut LunarWorld, *mut c_void);
pub type LunarQueryFn  = unsafe extern "C" fn(LunarEntity, *mut c_void);

// ─── internal registry resource ──────────────────────────────────────────────

#[derive(Clone)]
struct RegisteredSystem {
    callback:  LunarSystemFn,
    user_data: *mut c_void,
}

unsafe impl Send for RegisteredSystem {}
unsafe impl Sync for RegisteredSystem {}

/// world resource that holds the FFI component/system registries.
///
/// insert with [`init_registry`] before calling any `lunar_*` function.
#[derive(Resource, Default)]
pub struct FfiRegistry {
    component_names:     HashMap<String, ComponentId>,
    component_ids:       HashMap<LunarComponentId, ComponentId>,
    next_system_id:      LunarSystemId,
    startup_systems:     HashMap<LunarSystemId, RegisteredSystem>,
    update_systems:      HashMap<LunarSystemId, RegisteredSystem>,
    fixed_update_systems:HashMap<LunarSystemId, RegisteredSystem>,
    shutdown_systems:    HashMap<LunarSystemId, RegisteredSystem>,
    /// true while a hot-reload is in progress so C# Init can skip scene setup.
    pub is_reload: bool,
}

impl FfiRegistry {
    /// look up a component id by its registered name.
    pub fn component_id_by_name(&self, name: &str) -> Option<ComponentId> {
        self.component_names.get(name).copied()
    }

    fn systems(&self, schedule: LunarSchedule) -> &HashMap<LunarSystemId, RegisteredSystem> {
        match schedule {
            LunarSchedule::Startup     => &self.startup_systems,
            LunarSchedule::Update      => &self.update_systems,
            LunarSchedule::FixedUpdate => &self.fixed_update_systems,
            LunarSchedule::Shutdown    => &self.shutdown_systems,
        }
    }

    fn systems_mut(&mut self, schedule: LunarSchedule) -> &mut HashMap<LunarSystemId, RegisteredSystem> {
        match schedule {
            LunarSchedule::Startup     => &mut self.startup_systems,
            LunarSchedule::Update      => &mut self.update_systems,
            LunarSchedule::FixedUpdate => &mut self.fixed_update_systems,
            LunarSchedule::Shutdown    => &mut self.shutdown_systems,
        }
    }
}

// ─── Rust-side integration API ────────────────────────────────────────────────

/// insert [`FfiRegistry`] and cache built-in component names.
///
/// must be called after the engine's own plugins have registered their components.
pub fn init_registry(world: &mut World) {
    if world.contains_resource::<FfiRegistry>() { return; }
    world.insert_resource(FfiRegistry::default());
    cache_builtin(world, "LocalTransform3d", world.component_id::<LocalTransform3d>());
    cache_builtin(world, "WorldTransform3d", world.component_id::<WorldTransform3d>());
    cache_builtin(world, "LocalTransform2d", world.component_id::<LocalTransform>());
    cache_builtin(world, "WorldTransform2d", world.component_id::<WorldTransform>());
    cache_builtin(world, "Mesh3d",           world.component_id::<Mesh3d>());
    cache_builtin(world, "Camera3d",         world.component_id::<Camera3d>());
}

fn cache_builtin(world: &mut World, name: &str, id: Option<ComponentId>) {
    let Some(id) = id else { return };
    let ffi_id = id.index() as LunarComponentId;
    let mut reg = world.resource_mut::<FfiRegistry>();
    reg.component_names.insert(name.to_string(), id);
    reg.component_ids.insert(ffi_id, id);
}

/// clear all FFI systems registered for `schedule`. call before reloading a plugin.
pub fn clear_schedule(world: &mut World, schedule: LunarSchedule) {
    if let Some(mut reg) = world.get_resource_mut::<FfiRegistry>() {
        reg.systems_mut(schedule).clear();
    }
}

/// set the `is_reload` flag on the registry.
pub fn set_is_reload(world: &mut World, value: bool) {
    if let Some(mut reg) = world.get_resource_mut::<FfiRegistry>() {
        reg.is_reload = value;
    }
}

/// call all systems registered for `schedule`. invoke this from the engine game loop.
pub fn dispatch_systems(world: &mut World, schedule: LunarSchedule) {
    let systems: Vec<RegisteredSystem> = {
        let reg = world.resource::<FfiRegistry>();
        reg.systems(schedule).values().cloned().collect()
    };
    let world_ptr = world as *mut World as *mut LunarWorld;
    for sys in systems {
        unsafe { (sys.callback)(world_ptr, sys.user_data) };
    }
}

// ─── entity management ────────────────────────────────────────────────────────

/// spawn an empty entity and return its index.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_spawn(world: *mut LunarWorld) -> LunarEntity {
    let world = unsafe { world_from_ffi(world) };
    world.spawn_empty().id().index_u32()
}

/// despawn an entity by index. no-op if not found.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_despawn(world: *mut LunarWorld, entity: LunarEntity) {
    let world = unsafe { world_from_ffi(world) };
    let Some(e) = Entity::from_raw_u32(entity) else { return };
    world.despawn(e);
}

/// return true if the entity is alive in this world.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_alive(world: *mut LunarWorld, entity: LunarEntity) -> bool {
    let world = unsafe { world_from_ffi(world) };
    let Some(e) = Entity::from_raw_u32(entity) else { return false };
    world.get_entity(e).is_ok()
}

// ─── component registration ───────────────────────────────────────────────────

/// register a new component type by its memory layout.
///
/// `name` is a null-terminated UTF-8 string that is copied by the engine.
/// `size` and `alignment` must satisfy `Layout::from_size_align`.
/// returns [`LUNAR_INVALID_COMPONENT_ID`] on invalid layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_component_register(
    world:     *mut LunarWorld,
    name:      *const c_char,
    size:      usize,
    alignment: usize,
) -> LunarComponentId {
    let world = unsafe { world_from_ffi(world) };
    let name_str = unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned();

    let Ok(layout) = Layout::from_size_align(size, alignment) else {
        log::warn!("ffi: invalid layout for component '{name_str}' (size={size} align={alignment})");
        return LUNAR_INVALID_COMPONENT_ID;
    };

    // SAFETY: layout is valid; no drop needed; callers manage component memory
    let descriptor = unsafe {
        ComponentDescriptor::new_with_layout(
            name_str.clone(),
            StorageType::Table,
            layout,
            None,
            true,
            ComponentCloneBehavior::Default,
            None,
        )
    };

    let id = world.register_component_with_descriptor(descriptor);
    let ffi_id = id.index() as LunarComponentId;
    let mut reg = world.resource_mut::<FfiRegistry>();
    reg.component_names.insert(name_str, id);
    reg.component_ids.insert(ffi_id, id);
    ffi_id
}

/// look up a component id by name.
///
/// built-in names: `"LocalTransform3d"`, `"WorldTransform3d"`,
/// `"LocalTransform2d"`, `"WorldTransform2d"`.
/// returns [`LUNAR_INVALID_COMPONENT_ID`] if not found.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_component_id(
    world: *mut LunarWorld,
    name:  *const c_char,
) -> LunarComponentId {
    let world = unsafe { world_from_ffi(world) };
    let name_str = unsafe { CStr::from_ptr(name) }.to_str().unwrap_or("");

    if let Some(id) = world.resource::<FfiRegistry>().component_names.get(name_str) {
        return id.index() as LunarComponentId;
    }

    // lazy lookup for built-ins not yet cached in the registry
    let id = match name_str {
        "LocalTransform3d" => world.component_id::<LocalTransform3d>(),
        "WorldTransform3d" => world.component_id::<WorldTransform3d>(),
        "LocalTransform2d" => world.component_id::<LocalTransform>(),
        "WorldTransform2d" => world.component_id::<WorldTransform>(),
        _ => None,
    };

    match id {
        Some(id) => {
            let ffi_id = id.index() as LunarComponentId;
            let mut reg = world.resource_mut::<FfiRegistry>();
            reg.component_names.insert(name_str.to_string(), id);
            reg.component_ids.insert(ffi_id, id);
            ffi_id
        }
        None => LUNAR_INVALID_COMPONENT_ID,
    }
}

// ─── component access ─────────────────────────────────────────────────────────

/// insert (or replace) a component on an entity by copying `size` bytes from `data`.
///
/// `size` must match the size the component was registered with.
/// no-op if the entity or component id is unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_component_insert(
    world:        *mut LunarWorld,
    entity:       LunarEntity,
    component_id: LunarComponentId,
    data:         *const c_void,
    _size:        usize,
) {
    let world = unsafe { world_from_ffi(world) };

    let comp_id = {
        let reg = world.resource::<FfiRegistry>();
        match reg.component_ids.get(&component_id).copied() {
            Some(id) => id,
            None => {
                log::warn!("ffi: lunar_component_insert: unknown component id {component_id}");
                return;
            }
        }
    };

    let Some(entity) = Entity::from_raw_u32(entity) else { return };
    let Ok(mut entity_mut) = world.get_entity_mut(entity) else { return };

    // SAFETY: data is a valid pointer to a value matching the component layout.
    // the ECS copies the bytes via copy_nonoverlapping into its own storage.
    let ptr = unsafe { OwningPtr::new(NonNull::new_unchecked(data as *mut u8)) };
    unsafe { entity_mut.insert_by_id(comp_id, ptr) };
}

/// remove a component from an entity. no-op if not present.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_component_remove(
    world:        *mut LunarWorld,
    entity:       LunarEntity,
    component_id: LunarComponentId,
) {
    let world = unsafe { world_from_ffi(world) };
    let comp_id = {
        let reg = world.resource::<FfiRegistry>();
        match reg.component_ids.get(&component_id).copied() { Some(id) => id, None => return }
    };
    let Some(entity) = Entity::from_raw_u32(entity) else { return };
    let Ok(mut entity_mut) = world.get_entity_mut(entity) else { return };
    entity_mut.remove_by_id(comp_id);
}

/// return true if the entity has the component.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_component_has(
    world:        *mut LunarWorld,
    entity:       LunarEntity,
    component_id: LunarComponentId,
) -> bool {
    let world = unsafe { world_from_ffi(world) };
    let comp_id = {
        let reg = world.resource::<FfiRegistry>();
        match reg.component_ids.get(&component_id).copied() { Some(id) => id, None => return false }
    };
    let Some(entity) = Entity::from_raw_u32(entity) else { return false };
    match world.get_entity(entity) {
        Ok(entity_ref) => entity_ref.contains_id(comp_id),
        Err(_) => false,
    }
}

/// return a read-only pointer to the component, or null if not present.
///
/// the pointer is only valid until the current system callback returns.
/// for `LocalTransform3d` / `WorldTransform3d`, prefer the typed accessors.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_component_get(
    world:        *mut LunarWorld,
    entity:       LunarEntity,
    component_id: LunarComponentId,
) -> *const c_void {
    let world = unsafe { world_from_ffi(world) };
    let comp_id = {
        let reg = world.resource::<FfiRegistry>();
        match reg.component_ids.get(&component_id).copied() { Some(id) => id, None => return null() }
    };
    let Some(entity) = Entity::from_raw_u32(entity) else { return null() };
    let Ok(entity_ref) = world.get_entity(entity) else { return null() };
    match entity_ref.get_by_id(comp_id) {
        Ok(ptr) => ptr.as_ptr() as *const c_void,
        Err(_)  => null(),
    }
}

/// return a mutable pointer to the component, or null if not present.
///
/// the pointer is only valid until the current system callback returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_component_get_mut(
    world:        *mut LunarWorld,
    entity:       LunarEntity,
    component_id: LunarComponentId,
) -> *mut c_void {
    let world = unsafe { world_from_ffi(world) };
    let comp_id = {
        let reg = world.resource::<FfiRegistry>();
        match reg.component_ids.get(&component_id).copied() { Some(id) => id, None => return null_mut() }
    };
    let Some(entity) = Entity::from_raw_u32(entity) else { return null_mut() };
    let Ok(mut entity_mut) = world.get_entity_mut(entity) else { return null_mut() };
    match entity_mut.get_mut_by_id(comp_id) {
        Ok(mut_untyped) => mut_untyped.into_inner().as_ptr() as *mut c_void,
        Err(_)          => null_mut(),
    }
}

// ─── typed transform accessors ────────────────────────────────────────────────

/// read `LocalTransform3d` into a portable `LunarTransform3d` (handles SIMD alignment).
/// returns false if the entity or component is missing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_get_transform3d(
    world:  *mut LunarWorld,
    entity: LunarEntity,
    out:    *mut LunarTransform3d,
) -> bool {
    let world = unsafe { world_from_ffi(world) };
    let Some(entity) = Entity::from_raw_u32(entity) else { return false };
    let Ok(entity_ref) = world.get_entity(entity) else { return false };
    let Some(t) = entity_ref.get::<LocalTransform3d>() else { return false };
    unsafe {
        (*out).translation = LunarVec3 { x: t.translation.x, y: t.translation.y, z: t.translation.z };
        (*out).rotation    = LunarQuat { x: t.rotation.x, y: t.rotation.y, z: t.rotation.z, w: t.rotation.w };
        (*out).scale       = LunarVec3 { x: t.scale.x, y: t.scale.y, z: t.scale.z };
    }
    true
}

/// write a portable `LunarTransform3d` into `LocalTransform3d` (handles SIMD alignment).
/// returns false if the entity or component is missing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_set_transform3d(
    world:  *mut LunarWorld,
    entity: LunarEntity,
    value:  *const LunarTransform3d,
) -> bool {
    let world = unsafe { world_from_ffi(world) };
    let Some(entity) = Entity::from_raw_u32(entity) else { return false };
    let Ok(mut entity_mut) = world.get_entity_mut(entity) else { return false };
    let Some(mut t) = entity_mut.get_mut::<LocalTransform3d>() else { return false };
    let v = unsafe { &*value };
    t.translation = glam::Vec3::new(v.translation.x, v.translation.y, v.translation.z);
    t.rotation    = glam::Quat::from_xyzw(v.rotation.x, v.rotation.y, v.rotation.z, v.rotation.w);
    t.scale       = glam::Vec3::new(v.scale.x, v.scale.y, v.scale.z);
    true
}

/// read `LocalTransform` (2D) into a portable `LunarTransform2d`.
/// returns false if the entity or component is missing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_get_transform2d(
    world:  *mut LunarWorld,
    entity: LunarEntity,
    out:    *mut LunarTransform2d,
) -> bool {
    let world = unsafe { world_from_ffi(world) };
    let Some(entity) = Entity::from_raw_u32(entity) else { return false };
    let Ok(entity_ref) = world.get_entity(entity) else { return false };
    let Some(t) = entity_ref.get::<LocalTransform>() else { return false };
    unsafe {
        (*out).translation = LunarVec2 { x: t.translation.x, y: t.translation.y };
        (*out).rotation    = t.rotation;
        (*out).scale       = LunarVec2 { x: t.scale.x, y: t.scale.y };
    }
    true
}

/// write a portable `LunarTransform2d` into `LocalTransform` (2D).
/// returns false if the entity or component is missing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_set_transform2d(
    world:  *mut LunarWorld,
    entity: LunarEntity,
    value:  *const LunarTransform2d,
) -> bool {
    let world = unsafe { world_from_ffi(world) };
    let Some(entity) = Entity::from_raw_u32(entity) else { return false };
    let Ok(mut entity_mut) = world.get_entity_mut(entity) else { return false };
    let Some(mut t) = entity_mut.get_mut::<LocalTransform>() else { return false };
    let v = unsafe { &*value };
    t.translation = glam::Vec2::new(v.translation.x, v.translation.y);
    t.rotation    = v.rotation;
    t.scale       = glam::Vec2::new(v.scale.x, v.scale.y);
    true
}

// ─── query iteration ──────────────────────────────────────────────────────────

/// iterate all entities that have all `include` components and none of the `exclude` components.
///
/// `callback` is called once per matching entity with `user_data` passed through.
/// the world pointer passed to the callback is the same one this function received
/// and can be used to call any `lunar_*` API.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_query_foreach(
    world:         *mut LunarWorld,
    include:       *const LunarComponentId,
    include_count: usize,
    exclude:       *const LunarComponentId,
    exclude_count: usize,
    callback:      Option<unsafe extern "C" fn(LunarEntity, *mut c_void)>,
    user_data:     *mut c_void,
) {
    let Some(callback) = callback else { return };
    let world = unsafe { world_from_ffi(world) };

    let include_ids: Vec<ComponentId> = {
        let slice = unsafe { std::slice::from_raw_parts(include, include_count) };
        let reg = world.resource::<FfiRegistry>();
        slice.iter().filter_map(|id| reg.component_ids.get(id).copied()).collect()
    };
    let exclude_ids: Vec<ComponentId> = {
        let slice = unsafe { std::slice::from_raw_parts(exclude, exclude_count) };
        let reg = world.resource::<FfiRegistry>();
        slice.iter().filter_map(|id| reg.component_ids.get(id).copied()).collect()
    };

    // collect matching entity indices before calling callbacks (avoids borrow conflicts)
    let mut query_state = world.query::<EntityRef>();
    let matching: Vec<LunarEntity> = query_state.iter(world)
        .filter(|e| {
            include_ids.iter().all(|id| e.contains_id(*id))
                && exclude_ids.iter().all(|id| !e.contains_id(*id))
        })
        .map(|e| e.id().index_u32())
        .collect();

    let world_ptr = world as *mut World as *mut LunarWorld;
    for entity in matching {
        unsafe { callback(entity, user_data) };
        let _ = world_ptr; // keep alive
    }
}

// ─── system registration ──────────────────────────────────────────────────────

/// register a system callback for the given schedule.
///
/// `schedule` must be one of the `LUNAR_SCHEDULE_*` constants.
/// `user_data` is passed through to each call; the engine does not touch it.
/// returns [`LUNAR_INVALID_SYSTEM_ID`] on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_system_register(
    world:     *mut LunarWorld,
    schedule:  u32,
    callback:  Option<unsafe extern "C" fn(*mut LunarWorld, *mut c_void)>,
    user_data: *mut c_void,
) -> LunarSystemId {
    let Some(callback) = callback else { return LUNAR_INVALID_SYSTEM_ID };
    let Some(schedule) = LunarSchedule::from_u32(schedule) else {
        log::warn!("ffi: lunar_system_register: unknown schedule {schedule}");
        return LUNAR_INVALID_SYSTEM_ID;
    };

    let world = unsafe { world_from_ffi(world) };
    let mut reg = world.resource_mut::<FfiRegistry>();
    let id = reg.next_system_id;
    reg.next_system_id += 1;
    reg.systems_mut(schedule).insert(id, RegisteredSystem { callback, user_data });
    id
}

/// unregister a system by id. no-op if not found.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_system_unregister(world: *mut LunarWorld, id: LunarSystemId) {
    let world = unsafe { world_from_ffi(world) };
    let mut reg = world.resource_mut::<FfiRegistry>();
    reg.startup_systems.remove(&id);
    reg.update_systems.remove(&id);
    reg.fixed_update_systems.remove(&id);
    reg.shutdown_systems.remove(&id);
}

// ─── time ────────────────────────────────────────────────────────────────────

/// seconds elapsed since the previous frame.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_delta_seconds(world: *mut LunarWorld) -> f32 {
    let world = unsafe { world_from_ffi(world) };
    world.resource::<lunar_core::Time>().delta_seconds()
}

/// total seconds elapsed since the engine started.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_elapsed_seconds(world: *mut LunarWorld) -> f32 {
    let world = unsafe { world_from_ffi(world) };
    world.resource::<lunar_core::Time>().elapsed_seconds()
}

// ─── input ───────────────────────────────────────────────────────────────────

/// map a u32 discriminant to a KeyCode. discriminants match the Rust enum layout:
/// A-Z = 0-25, Num0-9 = 26-35, F1-12 = 36-47, Escape-Down = 48-56,
/// modifiers = 57-62, punctuation = 63-73, nav = 74-79, numpad = 80-96,
/// lock/super/media = 97-109, F13-24 = 128-139.
fn keycode_from_u32(value: u32) -> Option<KeyCode> {
    use KeyCode::*;
    const TABLE: &[KeyCode] = &[
        // A-Z (0-25)
        A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
        // Num0-9 (26-35)
        Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,
        // F1-F12 (36-47)
        F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
        // special (48-56)
        Escape, Space, Enter, Tab, Backspace, Left, Right, Up, Down,
        // modifiers (57-62)
        LShift, RShift, LCtrl, RCtrl, LAlt, RAlt,
        // punctuation (63-73)
        Minus, Equals, Semicolon, Apostrophe, Comma, Period, Slash, Backslash, LeftBracket, RightBracket, Grave,
        // navigation cluster (74-79)
        Home, End, PageUp, PageDown, Insert, Delete,
        // numpad (80-96)
        Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9,
        NumpadAdd, NumpadSub, NumpadMul, NumpadDiv, NumpadEnter, NumpadDecimal, NumLock,
        // lock/control (97-100)
        CapsLock, ScrollLock, Pause, PrintScreen,
        // super (101-102)
        LSuper, RSuper,
        // media (103-109)
        MediaPlay, MediaStop, MediaNext, MediaPrev, VolumeUp, VolumeDown, Mute,
    ];
    if value >= 128 {
        const EXT: &[KeyCode] = &[F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24];
        return EXT.get((value - 128) as usize).copied();
    }
    TABLE.get(value as usize).copied()
}

fn gamepad_axis_from_u32(value: u32) -> Option<GamepadAxis> {
    match value {
        0 => Some(GamepadAxis::LeftStickX),
        1 => Some(GamepadAxis::LeftStickY),
        2 => Some(GamepadAxis::RightStickX),
        3 => Some(GamepadAxis::RightStickY),
        4 => Some(GamepadAxis::LeftTrigger),
        5 => Some(GamepadAxis::RightTrigger),
        _ => None,
    }
}

/// return true if the key is currently held down.
/// `key` must be a `LUNAR_KEY_*` constant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_input_key_held(world: *mut LunarWorld, key: u32) -> bool {
    let world = unsafe { world_from_ffi(world) };
    let Some(keycode) = keycode_from_u32(key) else { return false };
    world.resource::<InputState>().is_key_held(keycode)
}

/// return true if the key was pressed this frame (edge-triggered).
/// `key` must be a `LUNAR_KEY_*` constant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_input_key_just_pressed(world: *mut LunarWorld, key: u32) -> bool {
    let world = unsafe { world_from_ffi(world) };
    let Some(keycode) = keycode_from_u32(key) else { return false };
    world.resource::<InputState>().is_key_just_pressed(keycode)
}

/// write the mouse movement delta for this frame into `*out_dx` and `*out_dy`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_input_mouse_delta(
    world:  *mut LunarWorld,
    out_dx: *mut f32,
    out_dy: *mut f32,
) {
    let world = unsafe { world_from_ffi(world) };
    let (dx, dy) = world.resource::<InputState>().mouse_delta();
    unsafe { *out_dx = dx; *out_dy = dy; }
}

/// return the current value of a gamepad axis (0.0 if gamepad not connected).
/// `axis` must be a `LUNAR_GAMEPAD_AXIS_*` constant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_input_gamepad_axis(
    world:         *mut LunarWorld,
    gamepad_index: u32,
    axis:          u32,
) -> f32 {
    let world = unsafe { world_from_ffi(world) };
    let Some(axis) = gamepad_axis_from_u32(axis) else { return 0.0 };
    world.resource::<InputState>()
        .gamepad(gamepad_index as usize)
        .map_or(0.0, |gp| gp.axis(axis))
}

/// return the entity index set by [`set_main_camera_entity`], or [`LUNAR_NULL_ENTITY`] if unset.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_get_main_camera(_world: *mut LunarWorld) -> LunarEntity {
    MAIN_CAMERA_ENTITY.load(Ordering::Relaxed)
}

// ─── scene setup ─────────────────────────────────────────────────────────────

/// sentinel for null/invalid asset handles.
pub const LUNAR_NULL_HANDLE: u64 = u64::MAX;

// pack Handle<T> (id: u32, generation: u16) into a u64.
// low 32 bits = id, bits 32..47 = generation.
fn pack_handle<T: Asset>(handle: Handle<T>) -> u64 {
    (handle.id() as u64) | ((handle.generation() as u64) << 32)
}

fn unpack_handle<T: Asset>(raw: u64) -> Handle<T> {
    Handle::new(raw as u32, (raw >> 32) as u16)
}

/// lock or unlock the cursor. mirrored into [`WindowSettings::cursor_locked`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_set_cursor_locked(world: *mut LunarWorld, locked: bool) {
    let world = unsafe { world_from_ffi(world) };
    if let Some(mut settings) = world.get_resource_mut::<WindowSettings>() {
        settings.cursor_locked = locked;
    }
}

/// insert or replace the [`Sky`] resource.
///
/// `sky_r/g/b` — skydome color (linear 0..1).
/// `sun_r/g/b` — sun disc color (linear 0..1).
/// `show_sun`  — whether to draw the sun disc.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_set_sky(
    world: *mut LunarWorld,
    sky_r: f32, sky_g: f32, sky_b: f32,
    sun_r: f32, sun_g: f32, sun_b: f32,
    show_sun: bool,
) {
    let world = unsafe { world_from_ffi(world) };
    world.insert_resource(Sky {
        sky_color: Color::rgb(sky_r, sky_g, sky_b),
        sun_color: Color::rgb(sun_r, sun_g, sun_b),
        show_sun,
        ..Sky::default()
    });
}

/// insert or replace [`QualitySettings`].
///
/// all other fields are taken from [`QualitySettings::minimum()`] as a base.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_set_quality(
    world:           *mut LunarWorld,
    msaa_samples:    u32,
    staa:            bool,
    render_scale:    f32,
    bloom:           bool,
    ssao:            bool,
    shadow_res:      u32,
    shadow_cascades: u32,
) {
    let world = unsafe { world_from_ffi(world) };
    world.insert_resource(QualitySettings {
        msaa_samples,
        staa,
        render_scale,
        bloom,
        ssao,
        shadow_res,
        shadow_cascades,
        ..QualitySettings::minimum()
    });
}

/// add a quad mesh to the registry and return a packed handle (u64).
///
/// `half_x` and `half_z` are the half-extents of the flat quad along X and Z.
/// returns [`LUNAR_NULL_HANDLE`] if [`MeshRegistry`] is not available.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_mesh_quad(
    world: *mut LunarWorld,
    half_x: f32,
    half_z: f32,
) -> u64 {
    let world = unsafe { world_from_ffi(world) };
    let Some(mut registry) = world.get_resource_mut::<MeshRegistry>() else { return LUNAR_NULL_HANDLE };
    pack_handle(registry.add_mesh(primitives::quad_mesh(half_x, half_z)))
}

/// add a box mesh to the registry and return a packed handle.
///
/// `hx`, `hy`, `hz` are the half-extents along each axis.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_mesh_box(
    world: *mut LunarWorld,
    hx: f32, hy: f32, hz: f32,
) -> u64 {
    let world = unsafe { world_from_ffi(world) };
    let Some(mut registry) = world.get_resource_mut::<MeshRegistry>() else { return LUNAR_NULL_HANDLE };
    pack_handle(registry.add_mesh(primitives::box_mesh(glam::Vec3::new(hx, hy, hz))))
}

/// add a UV sphere mesh to the registry and return a packed handle.
///
/// `sectors` and `stacks` control tessellation (minimum 3).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_mesh_sphere(
    world: *mut LunarWorld,
    radius: f32,
    sectors: u32,
    stacks: u32,
) -> u64 {
    let world = unsafe { world_from_ffi(world) };
    let Some(mut registry) = world.get_resource_mut::<MeshRegistry>() else { return LUNAR_NULL_HANDLE };
    pack_handle(registry.add_mesh(primitives::sphere_mesh(radius, sectors.max(3), stacks.max(3))))
}

/// add a cylinder mesh to the registry and return a packed handle.
///
/// `caps` controls whether the top and bottom disc faces are included.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_mesh_cylinder(
    world: *mut LunarWorld,
    radius: f32,
    height: f32,
    sectors: u32,
    caps: bool,
) -> u64 {
    let world = unsafe { world_from_ffi(world) };
    let Some(mut registry) = world.get_resource_mut::<MeshRegistry>() else { return LUNAR_NULL_HANDLE };
    pack_handle(registry.add_mesh(primitives::cylinder_mesh(radius, height, sectors.max(3), caps)))
}

/// create a material and return a packed handle.
///
/// `shading`: 0 = Unlit, 1 = Phong, 2 = Pbr.
/// returns [`LUNAR_NULL_HANDLE`] if [`MeshRegistry`] is not available.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_material_create(
    world: *mut LunarWorld,
    r: f32, g: f32, b: f32, a: f32,
    shading: u32,
) -> u64 {
    let world = unsafe { world_from_ffi(world) };
    let Some(mut registry) = world.get_resource_mut::<MeshRegistry>() else { return LUNAR_NULL_HANDLE };
    let shading_model = match shading {
        0 => ShadingModel::Unlit,
        2 => ShadingModel::Pbr,
        _ => ShadingModel::Phong,
    };
    pack_handle(registry.add_material(MaterialData {
        base_color: Color::rgba(r, g, b, a),
        shading: shading_model,
        ..MaterialData::default()
    }))
}

/// spawn a mesh entity at `(x, y, z)` with the given mesh and material handles.
///
/// handles must come from `lunar_mesh_*` and `lunar_material_create`.
/// returns the entity index, or [`LUNAR_NULL_ENTITY`] on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_spawn_mesh(
    world:      *mut LunarWorld,
    mesh_raw:   u64,
    mat_raw:    u64,
    x: f32, y: f32, z: f32,
) -> LunarEntity {
    let world = unsafe { world_from_ffi(world) };
    let mesh: Handle<MeshData>     = unpack_handle(mesh_raw);
    let mat:  Handle<MaterialData> = unpack_handle(mat_raw);
    world.spawn(Mesh3dBundle {
        local:    LocalTransform3d::from_xyz(x, y, z),
        mesh:     Mesh3d(mesh),
        material: Material3d(mat),
        ..Mesh3dBundle::default()
    }).id().index_u32()
}

/// spawn a perspective camera entity at `(x, y, z)`.
///
/// `fov_y` is in radians. returns the entity index.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_spawn_camera(
    world: *mut LunarWorld,
    x: f32, y: f32, z: f32,
    fov_y: f32, near: f32, far: f32,
) -> LunarEntity {
    let world = unsafe { world_from_ffi(world) };
    world.spawn(Camera3dBundle {
        local: LocalTransform3d::from_xyz(x, y, z),
        camera: Camera3d {
            projection: Projection::Perspective { fov_y, near, far },
            ..Camera3d::default()
        },
        ..Camera3dBundle::default()
    }).id().index_u32()
}

/// set the active camera to `entity`. equivalent to calling
/// [`set_main_camera_entity`] from Rust.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_set_active_camera(_world: *mut LunarWorld, entity: LunarEntity) {
    set_main_camera_entity(entity);
}

// ─── hot reload ───────────────────────────────────────────────────────────────

/// returns true if `lunar_plugin_init` is being called as part of a hot reload.
///
/// during a reload, [`FfiRegistry::is_reload`] is set to true by the loader
/// so that C# `Init` can skip one-time scene setup and only re-register systems.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lunar_is_reload(world: *mut LunarWorld) -> bool {
    let world = unsafe { world_from_ffi(world) };
    world.resource::<FfiRegistry>().is_reload
}
