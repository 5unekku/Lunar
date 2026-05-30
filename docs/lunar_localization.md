# engine_localization

localization system for multi-language support

provides a [`Localization`] resource that manages the current language
and loads per-language dialogue files and string tables.

# example

```ignore
use engine_localization::Localization;

let mut loc = Localization::new("en");
loc.load_language("fr", "locales/fr/dialogues");
loc.set_language("fr");
```

## Structs

### Localization

localization resource for managing the current language
and loading per-language content.

### LocalizationPlugin

localization plugin, registers the localization resource.
