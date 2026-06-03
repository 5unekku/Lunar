//! runtime reader for compiled game data tables.
//!
//! game data is authored as TOML and compiled to a binary blob at build time
//! by `lunar-gamedata-build`. embed the blob with `include_bytes!` and load
//! it at startup with [`GameData::from_binary`].
//!
//! # authoring format (TOML)
//!
//! ```toml
//! [[enemies]]
//! id = "goblin"
//! health = 100
//! speed = 2.5
//! sprite = "goblin.png"
//!
//! [[enemies]]
//! id = "orc"
//! health = 250
//! speed = 1.5
//!
//! [[items]]
//! id = "sword"
//! damage = 25
//! weight = 3.0
//! ```
//!
//! every record must have an `id` string field. duplicate ids within a table
//! are rejected at compile time.
//!
//! # loading at runtime
//!
//! ```ignore
//! static DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/gamedata.bin"));
//!
//! let gd = GameData::from_binary(DATA).expect("invalid gamedata blob");
//! let goblin_health = gd.get_int("enemies", "goblin", "health"); // Some(100)
//! ```

use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};

/// a single field value in a data record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataValue {
	/// interned string (resolved via the string table).
	Str(u32),
	Int(i64),
	Float(f64),
	Bool(bool),
}

/// a compiled record: a map of field name ids to values.
///
/// field names are interned into the shared string table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRecord {
	/// interned id string for this record
	pub id: u32,
	/// field name id → value
	pub fields: Vec<(u32, DataValue)>,
}

impl DataRecord {
	fn get(&self, field_id: u32) -> Option<&DataValue> {
		self.fields
			.iter()
			.find(|(k, _)| *k == field_id)
			.map(|(_, v)| v)
	}
}

/// a compiled table: an ordered list of records with a name-keyed index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTable {
	/// records in definition order
	pub records: Vec<DataRecord>,
	/// id string → index into `records` (for O(1) lookup)
	pub index: HashMap<u32, usize>,
}

impl DataTable {
	/// find a record by its interned id.
	#[must_use]
	pub fn get(&self, id: u32) -> Option<&DataRecord> {
		self.index.get(&id).and_then(|&i| self.records.get(i))
	}

	/// number of records in this table.
	#[must_use]
	pub fn len(&self) -> usize {
		self.records.len()
	}

	/// true when the table has no records.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.records.is_empty()
	}

	/// iterate over all records.
	pub fn iter(&self) -> impl Iterator<Item = &DataRecord> {
		self.records.iter()
	}
}

/// compiled game data: a set of named tables with a shared string table.
///
/// load via [`GameData::from_binary`] after embedding the blob at compile time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameData {
	/// interned string table: index = id, value = string
	pub strings: Vec<String>,
	/// table name → compiled table (table names are stored as raw strings)
	pub tables: HashMap<String, DataTable>,
}

impl GameData {
	/// deserialize from a compiled binary blob.
	/// # Errors
	/// returns an error if the bytes are not a valid compiled game data blob.
	pub fn from_binary(bytes: &[u8]) -> Result<Self, String> {
		bincode::deserialize(bytes).map_err(|e| format!("invalid gamedata blob: {e}"))
	}

	/// resolve an interned string id to its string.
	#[must_use]
	pub fn resolve(&self, id: u32) -> Option<&str> {
		self.strings.get(id as usize).map(String::as_str)
	}

	/// intern a lookup string against the string table.
	/// returns None if the string is not present (was never compiled in).
	#[must_use]
	pub fn string_id(&self, value: &str) -> Option<u32> {
		self.strings
			.iter()
			.position(|x| x == value)
			.and_then(|i| u32::try_from(i).ok())
	}

	/// get a table by name.
	#[must_use]
	pub fn table(&self, name: &str) -> Option<&DataTable> {
		self.tables.get(name)
	}

	/// look up a record by table name and record id string.
	#[must_use]
	pub fn record(&self, table: &str, id: &str) -> Option<&DataRecord> {
		let id_u32 = self.string_id(id)?;
		self.table(table)?.get(id_u32)
	}

	fn field_value(&self, table: &str, id: &str, field: &str) -> Option<&DataValue> {
		let record = self.record(table, id)?;
		let field_id = self.string_id(field)?;
		record.get(field_id)
	}

	/// get a string field value.
	#[must_use]
	pub fn get_str<'a>(&'a self, table: &str, id: &str, field: &str) -> Option<&'a str> {
		match self.field_value(table, id, field)? {
			DataValue::Str(sid) => self.resolve(*sid),
			_ => None,
		}
	}

	/// get an integer field value.
	#[must_use]
	pub fn get_int(&self, table: &str, id: &str, field: &str) -> Option<i64> {
		match self.field_value(table, id, field)? {
			DataValue::Int(n) => Some(*n),
			_ => None,
		}
	}

	/// get a float field value.
	#[must_use]
	pub fn get_float(&self, table: &str, id: &str, field: &str) -> Option<f64> {
		match self.field_value(table, id, field)? {
			DataValue::Float(f) => Some(*f),
			_ => None,
		}
	}

	/// get a boolean field value.
	#[must_use]
	pub fn get_bool(&self, table: &str, id: &str, field: &str) -> Option<bool> {
		match self.field_value(table, id, field)? {
			DataValue::Bool(b) => Some(*b),
			_ => None,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn make_test_data() -> GameData {
		let strings = vec![
			"goblin".to_string(),
			"sprite".to_string(),
			"goblin.png".to_string(),
			"health".to_string(),
			"speed".to_string(),
		];
		let record = DataRecord {
			id: 0, // "goblin"
			fields: vec![
				(1, DataValue::Str(2)),     // sprite = "goblin.png"
				(3, DataValue::Int(100)),   // health = 100
				(4, DataValue::Float(2.5)), // speed = 2.5
			],
		};
		let mut index = HashMap::default();
		index.insert(0u32, 0usize);
		let table = DataTable {
			records: vec![record],
			index,
		};
		let mut tables = HashMap::default();
		tables.insert("enemies".to_string(), table);
		GameData { strings, tables }
	}

	#[test]
	fn lookup_int() {
		let gd = make_test_data();
		assert_eq!(gd.get_int("enemies", "goblin", "health"), Some(100));
	}

	#[test]
	fn lookup_float() {
		let gd = make_test_data();
		assert!((gd.get_float("enemies", "goblin", "speed").unwrap() - 2.5).abs() < 1e-9);
	}

	#[test]
	fn lookup_str() {
		let gd = make_test_data();
		assert_eq!(
			gd.get_str("enemies", "goblin", "sprite"),
			Some("goblin.png")
		);
	}

	#[test]
	fn missing_table_returns_none() {
		let gd = make_test_data();
		assert!(gd.table("bosses").is_none());
	}

	#[test]
	fn missing_record_returns_none() {
		let gd = make_test_data();
		assert!(gd.record("enemies", "dragon").is_none());
	}

	#[test]
	fn binary_roundtrip() {
		let gd = make_test_data();
		let bytes = bincode::serialize(&gd).unwrap();
		let restored = GameData::from_binary(&bytes).unwrap();
		assert_eq!(restored.get_int("enemies", "goblin", "health"), Some(100));
		assert_eq!(
			restored.get_str("enemies", "goblin", "sprite"),
			Some("goblin.png")
		);
	}
}
