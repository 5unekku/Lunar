//! textbox component for rendering text with typewriter animation
//!
//! provides a [`Textbox`] component that can be attached to entities
//! for displaying text with optional typewriter-style animation.
//!
//! # example
//!
//! ```ignore
//! use engine_render::textbox::{Textbox, TypewriterState};
//! use engine_math::Vec2;
//!
//! let mut textbox = Textbox::new("hello, world!", Vec2::new(100.0, 100.0), Vec2::new(400.0, 100.0));
//! textbox.set_font(0, 24.0);
//! textbox.start_typewriter(0.05); // 50ms per character
//! ```

use engine_math::{Color, Vec2};

/// a textbox component for rendering text on screen.
///
/// contains the text content, position, size, font settings,
/// and optional typewriter animation state.
#[derive(Debug, Clone)]
pub struct Textbox {
    /// the full text content.
    pub text: String,
    /// position on screen (top-left corner).
    pub position: Vec2,
    /// size of the textbox area.
    pub size: Vec2,
    /// font handle id (references a loaded font).
    pub font_id: u32,
    /// font size in pixels.
    pub font_size: f32,
    /// text color.
    pub color: Color,
    /// background color (none for transparent).
    pub background_color: Option<Color>,
    /// padding inside the textbox.
    pub padding: f32,
    /// current typewriter state (none if fully visible).
    pub typewriter: Option<TypewriterState>,
}

/// state for typewriter animation.
///
/// tracks how many characters are currently visible
/// and the timing for revealing the next one.
#[derive(Debug, Clone)]
pub struct TypewriterState {
    /// how many characters are currently visible.
    pub visible_chars: usize,
    /// time in seconds between each character reveal.
    pub interval: f32,
    /// accumulated time since last reveal.
    pub accumulator: f32,
    /// whether the animation is complete.
    pub complete: bool,
}

impl Textbox {
    /// create a new textbox.
    pub fn new(text: &str, position: Vec2, size: Vec2) -> Self {
        Self {
            text: text.to_string(),
            position,
            size,
            font_id: 0,
            font_size: 16.0,
            color: Color::WHITE,
            background_color: None,
            padding: 8.0,
            typewriter: None,
        }
    }

    /// set the font for this textbox.
    pub fn set_font(&mut self, font_id: u32, font_size: f32) {
        self.font_id = font_id;
        self.font_size = font_size;
    }

    /// set the text color.
    pub fn set_color(&mut self, color: Color) {
        self.color = color;
    }

    /// set the background color.
    pub fn set_background(&mut self, color: Color) {
        self.background_color = Some(color);
    }

    /// set the padding.
    pub fn set_padding(&mut self, padding: f32) {
        self.padding = padding;
    }

    /// start the typewriter animation.
    /// `interval` is the time in seconds between each character reveal.
    pub fn start_typewriter(&mut self, interval: f32) {
        self.typewriter = Some(TypewriterState {
            visible_chars: 0,
            interval,
            accumulator: 0.0,
            complete: false,
        });
    }

    /// update the typewriter animation by the given delta time.
    /// returns true if the animation is still in progress.
    pub fn update_typewriter(&mut self, delta: f32) -> bool {
        let Some(state) = &mut self.typewriter else {
            return false;
        };

        if state.complete {
            return false;
        }

        state.accumulator += delta;

        while state.accumulator >= state.interval && state.visible_chars < self.text.len() {
            state.accumulator -= state.interval;
            state.visible_chars += 1;
        }

        if state.visible_chars >= self.text.len() {
            state.complete = true;
            false
        } else {
            true
        }
    }

    /// skip the typewriter animation and show all text.
    pub fn skip_typewriter(&mut self) {
        if let Some(state) = &mut self.typewriter {
            state.visible_chars = self.text.len();
            state.complete = true;
        }
    }

    /// get the currently visible text (for typewriter animation).
    pub fn visible_text(&self) -> &str {
        if let Some(state) = &self.typewriter {
            let char_count = state.visible_chars;
            self.text
                .char_indices()
                .nth(char_count)
                .map(|(idx, _)| &self.text[..idx])
                .unwrap_or(&self.text)
        } else {
            &self.text
        }
    }
}
