//! input subsystem via SDL3
//!
//! handles keyboard, mouse, gamepad input. exposes state through clean interfaces.

/// input manager, tracks current input state
pub struct InputManager {
    /// whether the input system is initialized
    #[allow(dead_code)]
    initialized: bool,
}

impl InputManager {
    /// create a new input manager
    pub fn new() -> Self {
        log::info!("input manager initialized");
        InputManager { initialized: true }
    }

    /// check if a key is currently pressed (stub)
    pub fn is_key_pressed(&self, _key_code: u32) -> bool {
        false
    }
}

impl Default for InputManager {
    fn default() -> Self {
        Self::new()
    }
}
