//! localization system for multi-language support
//!
//! provides a [`Localization`] resource that manages the current language
//! and loads per-language dialogue files and string tables.
//!
//! # example
//!
//! ```ignore
//! use lunar_localization::Localization;
//!
//! let mut loc = Localization::new("en");
//! loc.load_language("fr", "locales/fr/dialogues");
//! loc.set_language("fr");
//! ```

use std::borrow::Cow;
use std::collections::HashMap;

use bevy_ecs::prelude::*;

/// localization resource for managing the current language
/// and loading per-language content.
#[derive(Resource)]
pub struct Localization {
    /// the current language code (e.g. "en", "fr", "ja")
    current_language: String,
    /// available languages and their display names
    available_languages: HashMap<String, String>,
    /// per-language string tables
    string_tables: HashMap<String, HashMap<String, String>>,
    /// base path for locale directories
    locale_base_path: String,
}

impl Localization {
    /// create a new localization manager with the given default language.
    #[must_use]
    pub fn new(default_language: &str) -> Self {
        let mut available = HashMap::new();
        available.insert(default_language.to_string(), default_language.to_string());

        Self {
            current_language: default_language.to_string(),
            available_languages: available,
            string_tables: HashMap::new(),
            locale_base_path: "locales".to_string(),
        }
    }

    /// set the base path for locale directories.
    ///
    /// when set, relative paths passed to [`load_strings_from_file`](Self::load_strings_from_file)
    /// are resolved under this directory. absolute paths bypass it.
    ///
    /// example structure: `{base_path}/{lang_code}/strings.ron`
    pub fn set_locale_path(&mut self, path: &str) {
        self.locale_base_path = path.to_string();
    }

    /// register an available language with a display name.
    pub fn register_language(&mut self, code: &str, display_name: &str) {
        self.available_languages
            .insert(code.to_string(), display_name.to_string());
    }

    /// get the list of available languages.
    #[must_use]
    pub const fn available_languages(&self) -> &HashMap<String, String> {
        &self.available_languages
    }

    /// get the current language code.
    #[must_use]
    pub fn current_language(&self) -> &str {
        &self.current_language
    }

    /// switch to a different language.
    /// returns an error if the language is not registered.
    ///
    /// # Errors
    /// returns an error if the language code is not available.
    pub fn set_language(&mut self, code: &str) -> Result<(), String> {
        if !self.available_languages.contains_key(code) {
            return Err(format!("language '{code}' is not available"));
        }
        self.current_language = code.to_string();
        Ok(())
    }

    /// load a string table for a language from a RON file.
    /// the file should contain a map of string keys to translated values:
    /// ```ron
    /// { "greeting": "hello", "farewell": "goodbye" }
    /// ```
    /// if `path` is relative and a locale base path is set via [`set_locale_path`](Self::set_locale_path),
    /// the path is resolved as `{base_path}/{path}`.
    ///
    /// # Errors
    /// returns an error if the file cannot be read or the RON is invalid.
    pub fn load_strings_from_file(&mut self, lang: &str, path: &str) -> Result<(), String> {
        let full_path = if std::path::Path::new(path).is_absolute() || self.locale_base_path.is_empty() {
            path.to_string()
        } else {
            format!("{}/{}", self.locale_base_path, path)
        };
        let source = std::fs::read_to_string(&full_path)
            .map_err(|e| format!("failed to read strings file '{full_path}': {e}"))?;
        self.load_strings(lang, &source)
    }

    /// load a string table for a language from a RON string.
    /// # Errors
    /// returns an error if the RON source is invalid.
    pub fn load_strings(&mut self, lang: &str, source: &str) -> Result<(), String> {
        let strings: HashMap<String, String> =
            ron::from_str(source).map_err(|e| format!("ron parse error: {e}"))?;
        self.string_tables.insert(lang.to_string(), strings);
        self.available_languages
            .entry(lang.to_string())
            .or_insert_with(|| lang.to_string());
        Ok(())
    }

    /// get a localized string by key.
    ///
    /// returns `Cow::Borrowed` pointing into the string table when found — no allocation.
    /// falls back to `Cow::Borrowed(key)` on miss, also allocation-free.
    #[must_use]
    pub fn get<'a>(&'a self, key: &'a str) -> Cow<'a, str> {
        self.string_tables
            .get(&self.current_language)
            .and_then(|table| table.get(key))
            .map(|s| Cow::Borrowed(s.as_str()))
            .unwrap_or(Cow::Borrowed(key))
    }

    /// get a localized string by key with a fallback.
    ///
    /// returns `Cow::Borrowed` in all cases — no allocation.
    #[must_use]
    pub fn get_or<'a>(&'a self, key: &'a str, fallback: &'a str) -> Cow<'a, str> {
        self.string_tables
            .get(&self.current_language)
            .and_then(|table| table.get(key))
            .map(|s| Cow::Borrowed(s.as_str()))
            .unwrap_or(Cow::Borrowed(fallback))
    }
}

impl Default for Localization {
    fn default() -> Self {
        Self::new("en")
    }
}

/// localization plugin, registers the localization resource.
pub struct LocalizationPlugin {
    default_language: String,
}

impl LocalizationPlugin {
    /// create a new localization plugin with the given default language.
    #[must_use]
    pub fn new(default_language: &str) -> Self {
        Self {
            default_language: default_language.to_string(),
        }
    }
}

impl lunar_core::GamePlugin for LocalizationPlugin {
    fn name(&self) -> &'static str {
        "LocalizationPlugin"
    }

    fn dependencies(&self) -> &[&str] {
        &[]
    }

    fn build(&mut self, app: &mut lunar_core::App) {
        app.insert_resource(Localization::new(&self.default_language));
        log::info!(
            "LocalizationPlugin: default language '{}'",
            self.default_language
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localization_default_language() {
        let loc = Localization::new("en");
        assert_eq!(loc.current_language(), "en");
    }

    #[test]
    fn localization_register_language() {
        let mut loc = Localization::new("en");
        loc.register_language("fr", "French");
        assert!(loc.available_languages().contains_key("fr"));
    }

    #[test]
    fn localization_set_language() {
        let mut loc = Localization::new("en");
        loc.register_language("fr", "French");
        assert!(loc.set_language("fr").is_ok());
        assert_eq!(loc.current_language(), "fr");
    }

    #[test]
    fn localization_set_unknown_language_fails() {
        let mut loc = Localization::new("en");
        assert!(loc.set_language("xx").is_err());
    }

    #[test]
    fn localization_string_lookup() {
        let mut loc = Localization::new("en");
        loc.load_strings("en", r#"{ "greeting": "hello", "farewell": "goodbye" }"#)
            .unwrap();
        assert_eq!(loc.get("greeting"), "hello");
        assert_eq!(loc.get("unknown"), "unknown");
    }

    #[test]
    fn localization_fallback() {
        let loc = Localization::new("en");
        assert_eq!(loc.get_or("missing", "fallback"), "fallback");
    }
}

/// common, game-facing localization types for `use lunar::prelude::*`.
pub mod prelude {
    pub use crate::{Localization, LocalizationPlugin};
}
