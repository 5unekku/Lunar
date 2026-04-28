//! localization system for multi-language support
//!
//! provides a [`Localization`] resource that manages the current language
//! and loads per-language dialogue files and string tables.
//!
//! # example
//!
//! ```ignore
//! use engine_core::localization::Localization;
//!
//! let mut loc = Localization::new("en");
//! loc.load_language("fr", "locales/fr/dialogues");
//! loc.set_language("fr");
//! ```

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
    /// the engine expects a directory structure like:
    /// `{base_path}/{lang_code}/strings.yaml`
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

    /// load a string table for a language from a yaml file.
    /// the file should contain key-value pairs:
    /// ```yaml
    /// greeting: "hello"
    /// farewell: "goodbye"
    /// ```
    /// # Errors
    /// returns an error if the file cannot be read or if the yaml is invalid.
    pub fn load_strings_from_file(&mut self, lang: &str, path: &str) -> Result<(), String> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read strings file '{path}': {e}"))?;
        self.load_strings(lang, &source)
    }

    /// load a string table for a language from a yaml string.
    /// # Errors
    /// returns an error if the yaml source is invalid.
    pub fn load_strings(&mut self, lang: &str, source: &str) -> Result<(), String> {
        let strings: HashMap<String, String> =
            serde_yaml::from_str(source).map_err(|e| format!("yaml parse error: {e}"))?;
        self.string_tables.insert(lang.to_string(), strings);
        self.available_languages
            .entry(lang.to_string())
            .or_insert_with(|| lang.to_string());
        Ok(())
    }

    /// get a localized string by key.
    /// falls back to the key itself if the string is not found.
    #[must_use]
    pub fn get(&self, key: &str) -> String {
        self.string_tables
            .get(&self.current_language)
            .and_then(|table| table.get(key))
            .cloned()
            .unwrap_or_else(|| key.to_string())
    }

    /// get a localized string by key with a fallback.
    #[must_use]
    pub fn get_or(&self, key: &str, fallback: &str) -> String {
        self.string_tables
            .get(&self.current_language)
            .and_then(|table| table.get(key))
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
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

impl crate::GamePlugin for LocalizationPlugin {
    fn name(&self) -> &'static str {
        "LocalizationPlugin"
    }

    fn dependencies(&self) -> &[&str] {
        &[]
    }

    fn build(&mut self, app: &mut crate::app::App) {
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
        loc.load_strings("en", "greeting: hello\nfarewell: goodbye")
            .unwrap();
        assert_eq!(loc.get("greeting"), "hello".to_string());
        assert_eq!(loc.get("unknown"), "unknown".to_string());
    }

    #[test]
    fn localization_fallback() {
        let loc = Localization::new("en");
        assert_eq!(loc.get_or("missing", "fallback"), "fallback".to_string());
    }
}
