//! build-time TOML compiler for lunar game data tables.
//!
//! call from your crate's `build.rs` to compile TOML source files into binary
//! blobs that can be loaded at runtime by [`lunar_gamedata::GameData`].
//!
//! # build.rs example
//!
//! ```ignore
//! fn main() {
//!     let src = std::fs::read_to_string("assets/data/enemies.toml").unwrap();
//!     let blob = lunar_gamedata_build::compile_toml(&src).unwrap();
//!     let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
//!     std::fs::write(out.join("gamedata.bin"), blob).unwrap();
//!     println!("cargo:rerun-if-changed=assets/data/enemies.toml");
//! }
//! ```
//!
//! # TOML format
//!
//! use array-of-tables syntax. every record must contain an `id` string field.
//! duplicate ids within a table are compile errors.
//!
//! ```toml
//! [[enemies]]
//! id = "goblin"
//! health = 100
//! speed = 2.5
//! sprite = "goblin.png"
//! boss = false
//!
//! [[enemies]]
//! id = "orc"
//! health = 250
//! speed = 1.5
//! ```
//!
//! supported TOML value types: `String`, `Integer`, `Float`, `Boolean`.
//! arrays, inline tables, and datetimes are not supported.

use std::collections::HashMap;

use lunar_gamedata::{DataRecord, DataTable, DataValue, GameData};

/// compile a TOML source string into a binary blob.
///
/// the blob can be loaded at runtime via [`GameData::from_binary`].
///
/// # Errors
///
/// returns an error string if:
/// - the TOML source fails to parse
/// - a record is missing the required `id` field
/// - duplicate ids exist within the same table
/// - a field value type is not supported (array, inline table, datetime)
pub fn compile_toml(source: &str) -> Result<Vec<u8>, String> {
    let root: toml::Value =
        source.parse().map_err(|e| format!("toml parse error: {e}"))?;

    let table_map = root.as_table().ok_or("root toml value must be a table")?;

    let mut strings: Vec<String> = Vec::new();
    let mut string_index: HashMap<String, u32> = HashMap::new();
    let mut tables: HashMap<String, DataTable> = HashMap::new();

    let mut intern = |s: &str, strings: &mut Vec<String>, index: &mut HashMap<String, u32>| -> u32 {
        if let Some(&id) = index.get(s) {
            return id;
        }
        let id = strings.len() as u32;
        strings.push(s.to_string());
        index.insert(s.to_string(), id);
        id
    };

    for (table_name, value) in table_map {
        let records_toml = match value.as_array() {
            Some(arr) => arr,
            None => continue, // skip non-array top-level keys
        };

        // check first element to see if it's a table (array-of-tables)
        let is_table_array = records_toml.first().map_or(false, |v| v.is_table());
        if !is_table_array {
            continue;
        }

        let mut records: Vec<DataRecord> = Vec::new();
        let mut id_seen: HashMap<String, usize> = HashMap::new();

        for (record_index, record_value) in records_toml.iter().enumerate() {
            let fields_toml = record_value
                .as_table()
                .ok_or_else(|| format!("record {record_index} in table '{table_name}' is not a TOML table"))?;

            // require 'id' field
            let id_str = fields_toml
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    format!("record {record_index} in table '{table_name}' is missing required 'id' string field")
                })?;

            if let Some(prev) = id_seen.get(id_str) {
                return Err(format!(
                    "duplicate id '{id_str}' in table '{table_name}' (first at record {prev}, again at record {record_index})"
                ));
            }
            id_seen.insert(id_str.to_string(), record_index);

            let id_interned = intern(id_str, &mut strings, &mut string_index);

            let mut field_vec: Vec<(u32, DataValue)> = Vec::new();
            for (field_name, field_value) in fields_toml {
                if field_name == "id" {
                    continue; // id is not stored as a field
                }
                let field_id = intern(field_name, &mut strings, &mut string_index);
                let data_value = toml_to_data_value(field_value, field_name, table_name, &mut strings, &mut string_index)?;
                field_vec.push((field_id, data_value));
            }

            records.push(DataRecord {
                id: id_interned,
                fields: field_vec,
            });
        }

        let mut index: HashMap<u32, usize> = HashMap::new();
        for (i, record) in records.iter().enumerate() {
            index.insert(record.id, i);
        }

        tables.insert(table_name.clone(), DataTable { records, index });
    }

    let game_data = GameData { strings, tables };
    bincode::serialize(&game_data).map_err(|e| format!("serialization error: {e}"))
}

fn toml_to_data_value(
    value: &toml::Value,
    field_name: &str,
    table_name: &str,
    strings: &mut Vec<String>,
    string_index: &mut HashMap<String, u32>,
) -> Result<DataValue, String> {
    match value {
        toml::Value::String(s) => {
            let id = if let Some(&existing) = string_index.get(s.as_str()) {
                existing
            } else {
                let id = strings.len() as u32;
                strings.push(s.clone());
                string_index.insert(s.clone(), id);
                id
            };
            Ok(DataValue::Str(id))
        }
        toml::Value::Integer(n) => Ok(DataValue::Int(*n)),
        toml::Value::Float(f) => Ok(DataValue::Float(*f)),
        toml::Value::Boolean(b) => Ok(DataValue::Bool(*b)),
        _ => Err(format!(
            "unsupported value type for field '{field_name}' in table '{table_name}': \
             only string, integer, float, and boolean are supported"
        )),
    }
}

/// compile a TOML file to a binary blob.
///
/// a convenience wrapper around [`compile_toml`] for use in `build.rs`.
///
/// # Errors
///
/// returns an error string if the file cannot be read or compilation fails.
pub fn compile_toml_file(path: &str) -> Result<Vec<u8>, String> {
    let source =
        std::fs::read_to_string(path).map_err(|e| format!("failed to read '{path}': {e}"))?;
    compile_toml(&source)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[[enemies]]
id = "goblin"
health = 100
speed = 2.5
boss = false
sprite = "goblin.png"

[[enemies]]
id = "orc"
health = 250
speed = 1.5
boss = false

[[items]]
id = "sword"
damage = 25
weight = 3.0
"#;

    #[test]
    fn compile_and_read() {
        let blob = compile_toml(SAMPLE).unwrap();
        let gd = GameData::from_binary(&blob).unwrap();

        assert_eq!(gd.get_int("enemies", "goblin", "health"), Some(100));
        assert!((gd.get_float("enemies", "goblin", "speed").unwrap() - 2.5).abs() < 1e-9);
        assert_eq!(gd.get_bool("enemies", "goblin", "boss"), Some(false));
        assert_eq!(gd.get_str("enemies", "goblin", "sprite"), Some("goblin.png"));

        assert_eq!(gd.get_int("enemies", "orc", "health"), Some(250));
        assert_eq!(gd.get_float("items", "sword", "damage"), None);
        assert_eq!(gd.get_int("items", "sword", "damage"), Some(25));
    }

    #[test]
    fn missing_id_is_error() {
        let bad = "[[enemies]]\nhealth = 100\n";
        assert!(compile_toml(bad).is_err());
    }

    #[test]
    fn duplicate_id_is_error() {
        let bad = "[[enemies]]\nid = \"goblin\"\n[[enemies]]\nid = \"goblin\"\n";
        assert!(compile_toml(bad).is_err());
    }

    #[test]
    fn unsupported_value_type_is_error() {
        let bad = "[[things]]\nid = \"x\"\ntags = [\"a\", \"b\"]\n";
        assert!(compile_toml(bad).is_err());
    }

    #[test]
    fn empty_table_compiles() {
        let blob = compile_toml("").unwrap();
        let gd = GameData::from_binary(&blob).unwrap();
        assert!(gd.table("anything").is_none());
    }

    #[test]
    fn table_len() {
        let blob = compile_toml(SAMPLE).unwrap();
        let gd = GameData::from_binary(&blob).unwrap();
        assert_eq!(gd.table("enemies").unwrap().len(), 2);
        assert_eq!(gd.table("items").unwrap().len(), 1);
    }
}
