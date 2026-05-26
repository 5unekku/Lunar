//! screen-space UI components for in-game menus, HUD, and dialogue boxes.
//!
//! all positions are screen-space pixels (top-left origin, y-down).
//! UI draws on [`lunar_render::layers::UI`] (z=300), above game objects.
//!
//! # components
//!
//! - [`UiPanel`] — filled rect with optional border
//! - [`UiLabel`] — text label with optional word-wrap
//! - [`UiImage`] — texture display
//! - [`UiProgressBar`] — health/mana/XP bar with background + fill
//! - [`UiButton`] — clickable panel + label with hover/press tinting
//!
//! # interaction
//!
//! spawn a [`UiButton`] entity and optionally attach an [`Interaction`] component.
//! the [`ui_interaction_system`] updates `Interaction` each frame from mouse input
//! and fires [`ButtonPressedEvent`] on click release.
//!
//! # usage
//!
//! ```ignore
//! use lunar_ui::{UiPanel, UiLabel, UiProgressBar, UiButton, UiPlugin};
//! use lunar_math::{Vec2, Color};
//!
//! fn setup(mut commands: Commands, assets: Res<AssetServer>) {
//!     let font = assets.load_font("ui.ttf");
//!     // dialogue box background
//!     commands.spawn(UiPanel {
//!         position: Vec2::new(0.0, 480.0),
//!         size: Vec2::new(800.0, 120.0),
//!         color: Color::rgba(0.0, 0.0, 0.0, 0.8),
//!         border: None,
//!     });
//!     // health bar
//!     commands.spawn(UiProgressBar {
//!         position: Vec2::new(10.0, 10.0),
//!         size: Vec2::new(200.0, 20.0),
//!         fill: 0.75,
//!         background_color: Color::rgba(0.2, 0.0, 0.0, 1.0),
//!         fill_color: Color::rgb(0.8, 0.1, 0.1),
//!     });
//!     // menu button
//!     commands.spawn((
//!         UiButton {
//!             position: Vec2::new(300.0, 200.0),
//!             size: Vec2::new(200.0, 40.0),
//!             label: "Attack".into(),
//!             font: font.clone(),
//!             font_size: 18.0,
//!             text_color: Color::WHITE,
//!             normal_color: Color::rgba(0.15, 0.15, 0.3, 1.0),
//!             hover_color: Color::rgba(0.25, 0.25, 0.5, 1.0),
//!             press_color: Color::rgba(0.35, 0.35, 0.65, 1.0),
//!         },
//!         Interaction::None,
//!     ));
//! }
//! ```

use bevy_ecs::message::{Message, MessageRegistry, MessageWriter};
use bevy_ecs::prelude::*;
use lunar_assets::{Font, Handle, Texture};
use lunar_core::{App, GamePlugin};
use lunar_input::{InputState, MouseButton};
use lunar_math::{Color, Vec2};
use lunar_render::{Camera, RenderInfo, RenderQueue};

// ── primitive components ─────────────────────────────────────────────────────

/// screen-space filled rect. draws at [`lunar_render::layers::UI`].
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
    /// optional border: `(thickness_px, color)`.
    pub border: Option<(f32, Color)>,
}

/// screen-space text label. draws at [`lunar_render::layers::UI`].
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

/// screen-space image. draws at [`lunar_render::layers::UI`].
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

// ── progress bar ─────────────────────────────────────────────────────────────

/// screen-space progress bar for health, mana, XP, timers, etc.
///
/// draws a background rect and a foreground fill rect sized to `fill` [0.0, 1.0].
/// position is the top-left corner in screen pixels.
#[derive(Debug, Clone, Component)]
pub struct UiProgressBar {
    /// top-left corner in screen-space pixels.
    pub position: Vec2,
    /// total width and height in pixels.
    pub size: Vec2,
    /// fill fraction in [0.0, 1.0]. 0.0 = empty, 1.0 = full.
    pub fill: f32,
    /// background rect color.
    pub background_color: Color,
    /// foreground fill color.
    pub fill_color: Color,
}

// ── button ───────────────────────────────────────────────────────────────────

/// screen-space clickable button. draws at [`lunar_render::layers::UI`].
///
/// renders as a panel + centered label. color changes based on [`Interaction`] state.
/// attach an [`Interaction`] component to enable hover/press feedback and receive
/// [`ButtonPressedEvent`] events.
#[derive(Debug, Clone, Component)]
pub struct UiButton {
    /// top-left corner in screen-space pixels.
    pub position: Vec2,
    /// width and height in pixels.
    pub size: Vec2,
    /// button text.
    pub label: String,
    /// font handle.
    pub font: Handle<Font>,
    /// font size in pixels.
    pub font_size: f32,
    /// label text color.
    pub text_color: Color,
    /// background color when not hovered or pressed.
    pub normal_color: Color,
    /// background color on hover.
    pub hover_color: Color,
    /// background color when pressed.
    pub press_color: Color,
}

// ── interaction ───────────────────────────────────────────────────────────────

/// mouse interaction state for UI elements.
///
/// attach this component alongside [`UiButton`] to opt into mouse hit-testing.
/// the [`ui_interaction_system`] updates this each frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub enum Interaction {
    /// cursor is not over the element.
    None,
    /// cursor is hovering over the element.
    Hovered,
    /// primary mouse button is held down over the element.
    Pressed,
}

/// fired when a [`UiButton`] with [`Interaction`] is clicked (released while hovered).
///
/// read with `MessageReader<ButtonPressedEvent>` in your systems.
#[derive(Debug, Clone, Message)]
pub struct ButtonPressedEvent {
    /// the entity whose button was pressed.
    pub entity: Entity,
}

// ── systems ───────────────────────────────────────────────────────────────────

/// renders all [`UiPanel`], [`UiLabel`], and [`UiImage`] entities.
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
        if let Some((thickness, border_color)) = panel.border {
            draw_border(
                &mut queue,
                panel.position,
                panel.size,
                thickness,
                border_color,
                camera_ref,
                w,
                h,
            );
        }
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
                lunar_render::layers::UI,
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

/// renders all [`UiProgressBar`] entities.
pub fn render_progress_bars(
    bars: Query<&UiProgressBar>,
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

    for bar in &bars {
        // background
        queue.draw_ui_rect(
            bar.position,
            bar.size,
            bar.background_color,
            camera_ref,
            w,
            h,
        );

        // fill
        let fill = bar.fill.clamp(0.0, 1.0);
        if fill > 0.0 {
            let fill_size = Vec2::new(bar.size.x * fill, bar.size.y);
            queue.draw_ui_rect(bar.position, fill_size, bar.fill_color, camera_ref, w, h);
        }
    }
}

/// renders all [`UiButton`] entities using their current [`Interaction`] state.
pub fn render_buttons(
    buttons: Query<(&UiButton, Option<&Interaction>)>,
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

    for (button, interaction) in &buttons {
        let bg_color = match interaction {
            Some(Interaction::Pressed) => button.press_color,
            Some(Interaction::Hovered) => button.hover_color,
            _ => button.normal_color,
        };
        queue.draw_ui_rect(button.position, button.size, bg_color, camera_ref, w, h);

        // label offset: 8px left padding, vertically centered by eye
        let label_pos = Vec2::new(
            button.position.x + 8.0,
            button.position.y + (button.size.y - button.font_size) * 0.5,
        );
        queue.draw_ui_text(
            &button.font,
            &button.label,
            label_pos,
            button.font_size,
            button.text_color,
            camera_ref,
            w,
            h,
        );
    }
}

/// updates [`Interaction`] on [`UiButton`] entities and fires [`ButtonPressedEvent`].
///
/// requires [`InputState`] resource. runs in the [`UpdateStage::Update`] stage so
/// interaction state is set before the render stage reads it.
pub fn ui_interaction_system(
    input: Option<Res<InputState>>,
    mut buttons: Query<(Entity, &UiButton, &mut Interaction)>,
    mut events: MessageWriter<ButtonPressedEvent>,
) {
    let Some(input) = input else { return };
    let (mx, my) = input.mouse_position();

    for (entity, button, mut interaction) in &mut buttons {
        let hovered = mx >= button.position.x
            && mx <= button.position.x + button.size.x
            && my >= button.position.y
            && my <= button.position.y + button.size.y;

        let was_pressed = *interaction == Interaction::Pressed;

        *interaction = if hovered && input.is_mouse_button_held(MouseButton::Left) {
            Interaction::Pressed
        } else if hovered {
            Interaction::Hovered
        } else {
            Interaction::None
        };

        // fire on release inside bounds
        if was_pressed && hovered && input.is_mouse_button_just_released(MouseButton::Left) {
            events.write(ButtonPressedEvent { entity });
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// draws four border rects around a panel area.
fn draw_border(
    queue: &mut RenderQueue,
    position: Vec2,
    size: Vec2,
    thickness: f32,
    color: Color,
    camera: &Camera,
    w: u32,
    h: u32,
) {
    let t = thickness;
    // top
    queue.draw_ui_rect(position, Vec2::new(size.x, t), color, camera, w, h);
    // bottom
    queue.draw_ui_rect(
        Vec2::new(position.x, position.y + size.y - t),
        Vec2::new(size.x, t),
        color,
        camera,
        w,
        h,
    );
    // left
    queue.draw_ui_rect(position, Vec2::new(t, size.y), color, camera, w, h);
    // right
    queue.draw_ui_rect(
        Vec2::new(position.x + size.x - t, position.y),
        Vec2::new(t, size.y),
        color,
        camera,
        w,
        h,
    );
}

// ── plugin ────────────────────────────────────────────────────────────────────

/// plugin that registers all UI render and interaction systems.
pub struct UiPlugin;

impl GamePlugin for UiPlugin {
    fn name(&self) -> &'static str {
        "ui"
    }

    fn build(&mut self, app: &mut App) {
        MessageRegistry::register_message::<ButtonPressedEvent>(app.world_mut());
        app.add_system_to_stage(lunar_core::UpdateStage::Update, ui_interaction_system);
        app.add_system_to_stage(lunar_core::UpdateStage::Render, render_ui);
        app.add_system_to_stage(lunar_core::UpdateStage::Render, render_progress_bars);
        app.add_system_to_stage(lunar_core::UpdateStage::Render, render_buttons);
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lunar_assets::Handle;

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
            border: Some((2.0, Color::rgb(1.0, 1.0, 0.0))),
        };
        assert_eq!(panel.position, Vec2::new(10.0, 20.0));
        assert!(panel.border.is_some());
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

    #[test]
    fn ui_progress_bar_clamp() {
        let bar = UiProgressBar {
            position: Vec2::ZERO,
            size: Vec2::new(200.0, 20.0),
            fill: 1.5,
            background_color: Color::BLACK,
            fill_color: Color::WHITE,
        };
        assert!(bar.fill.clamp(0.0, 1.0) <= 1.0);
    }

    #[test]
    fn ui_button_fields() {
        let button = UiButton {
            position: Vec2::ZERO,
            size: Vec2::new(120.0, 40.0),
            label: "OK".into(),
            font: dummy_font(),
            font_size: 18.0,
            text_color: Color::WHITE,
            normal_color: Color::BLACK,
            hover_color: Color::rgb(0.2, 0.2, 0.2),
            press_color: Color::rgb(0.4, 0.4, 0.4),
        };
        assert_eq!(button.label, "OK");
    }

    #[test]
    fn interaction_default() {
        let i = Interaction::None;
        assert_eq!(i, Interaction::None);
    }
}
