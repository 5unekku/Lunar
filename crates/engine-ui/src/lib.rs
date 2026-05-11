//! screen-space UI components for in-game menus, HUD, and dialogue boxes.
//!
//! all positions are screen-space pixels (top-left origin, y-down).
//! UI draws on [`engine_render::layers::UI`] (z=300), above game objects.
//!
//! # usage
//!
//! ```ignore
//! use engine_ui::{UiPanel, UiLabel, UiImage, UiPlugin};
//! use engine_math::{Vec2, Color};
//!
//! fn setup(mut commands: Commands, assets: Res<AssetServer>) {
//!     let font = assets.get_font_handle("ui.ttf");
//!     // dialogue box background at the bottom of an 800x600 window
//!     commands.spawn(UiPanel {
//!         position: Vec2::new(0.0, 480.0),
//!         size: Vec2::new(800.0, 120.0),
//!         color: Color::rgba(0.0, 0.0, 0.0, 0.8),
//!     });
//!     commands.spawn(UiLabel {
//!         position: Vec2::new(16.0, 496.0),
//!         text: "Hello, world!".into(),
//!         font: font.clone(),
//!         font_size: 18.0,
//!         color: Color::WHITE,
//!         max_width: Some(760.0),
//!         line_height: 0.0,
//!     });
//! }
//! ```

use bevy_ecs::prelude::*;
use engine_assets::{Font, Handle, Texture};
use engine_core::{App, GamePlugin};
use engine_math::{Color, Vec2};
use engine_render::{Camera, RenderInfo, RenderQueue};

/// screen-space filled rect. draws at [`engine_render::layers::UI`].
///
/// position is the top-left corner in screen pixels.
#[derive(Debug, Clone, Component)]
pub struct UiPanel {
    /// top-left corner in screen-space pixels.
    pub position: Vec2,
    /// width and height in pixels.
    pub size: Vec2,
    /// fill color (supports transparency).
    pub color: Color,
}

/// screen-space text label. draws at [`engine_render::layers::UI`].
///
/// if `max_width` is set, text word-wraps at that pixel width.
#[derive(Debug, Clone, Component)]
pub struct UiLabel {
    /// top-left of the first line, in screen-space pixels.
    pub position: Vec2,
    /// the text to display.
    pub text: String,
    /// font handle.
    pub font: Handle<Font>,
    /// font size in pixels.
    pub font_size: f32,
    /// text color.
    pub color: Color,
    /// word-wrap width in pixels. `None` = single line.
    pub max_width: Option<f32>,
    /// line spacing when wrapping; 0.0 = font_size * 1.25.
    pub line_height: f32,
}

/// screen-space image. draws at [`engine_render::layers::UI`].
///
/// position is the top-left corner in screen pixels.
#[derive(Debug, Clone, Component)]
pub struct UiImage {
    /// top-left corner in screen-space pixels.
    pub position: Vec2,
    /// display size in pixels (stretched if different from source).
    pub size: Vec2,
    /// texture to display.
    pub texture: Handle<Texture>,
}

/// system that renders all UI components via [`RenderQueue`].
pub fn render_ui(
    panels: Query<&UiPanel>,
    labels: Query<&UiLabel>,
    images: Query<&UiImage>,
    camera: Option<Res<Camera>>,
    render_info: Res<RenderInfo>,
    mut queue: ResMut<RenderQueue>,
) {
    let (w, h) = (render_info.window_width, render_info.window_height);
    let default_camera;
    let camera_ref = match &camera {
        Some(cam) => cam.as_ref(),
        None => {
            default_camera = Camera::new();
            &default_camera
        }
    };

    for panel in &panels {
        queue.draw_ui_rect(panel.position, panel.size, panel.color, camera_ref, w, h);
    }

    for label in &labels {
        if let Some(max_width) = label.max_width {
            let world = camera_ref.screen_to_world(label.position, w, h);
            queue.draw_text_wrapped(
                &label.font,
                &label.text,
                world,
                label.font_size,
                label.color,
                max_width,
                label.line_height,
                engine_render::layers::UI,
            );
        } else {
            queue.draw_ui_text(
                &label.font,
                &label.text,
                label.position,
                label.font_size,
                label.color,
                camera_ref,
                w,
                h,
            );
        }
    }

    for image in &images {
        queue.draw_ui_sprite(&image.texture, image.position, image.size, camera_ref, w, h);
    }
}

/// plugin that registers the UI render system.
pub struct UiPlugin;

impl GamePlugin for UiPlugin {
    fn name(&self) -> &'static str {
        "ui"
    }

    fn build(&mut self, app: &mut App) {
        app.add_system_to_stage(engine_core::UpdateStage::Render, render_ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_assets::Handle;

    fn dummy_font() -> Handle<Font> {
        Handle::new(0, 0)
    }

    fn dummy_texture() -> Handle<Texture> {
        Handle::new(0, 0)
    }

    #[test]
    fn ui_panel_fields() {
        let panel = UiPanel {
            position: Vec2::new(10.0, 20.0),
            size: Vec2::new(200.0, 100.0),
            color: Color::WHITE,
        };
        assert_eq!(panel.position, Vec2::new(10.0, 20.0));
    }

    #[test]
    fn ui_label_fields() {
        let label = UiLabel {
            position: Vec2::ZERO,
            text: "hello".into(),
            font: dummy_font(),
            font_size: 16.0,
            color: Color::WHITE,
            max_width: Some(300.0),
            line_height: 0.0,
        };
        assert_eq!(label.text, "hello");
        assert_eq!(label.max_width, Some(300.0));
    }

    #[test]
    fn ui_image_fields() {
        let image = UiImage {
            position: Vec2::new(5.0, 5.0),
            size: Vec2::new(64.0, 64.0),
            texture: dummy_texture(),
        };
        assert_eq!(image.size, Vec2::new(64.0, 64.0));
    }
}
