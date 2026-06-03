//! game save/load — serializes game-owned data to RON on native, stub on WASM.
//!
//! the engine owns file I/O; the game owns the schema. any `Serialize +
//! DeserializeOwned` type can be round-tripped. paths are relative to the
//! game's save directory (resolved by the engine, not the caller).
//!
//! # example
//!
//! ```ignore
//! use lunar_core::persist::{self, PersistError};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct SaveData { level: u32, score: u64 }
//!
//! fn save_game(data: &SaveData) -> Result<(), PersistError> {
//!     persist::save("save0.ron", data)
//! }
//!
//! fn load_game() -> Result<SaveData, PersistError> {
//!     persist::load("save0.ron")
//! }
//! ```

use serde::{Serialize, de::DeserializeOwned};

/// errors returned by persist operations.
#[derive(Debug)]
pub enum PersistError {
	/// platform does not support file I/O (wasm)
	NotSupported,
	/// path could not be created or written
	Io(std::io::Error),
	/// serialization failed
	Serialize(ron::Error),
	/// deserialization failed
	Deserialize(ron::de::SpannedError),
}

impl std::fmt::Display for PersistError {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			PersistError::NotSupported => {
				formatter.write_str("persist: not supported on this platform")
			}
			PersistError::Io(error) => write!(formatter, "persist io: {error}"),
			PersistError::Serialize(error) => write!(formatter, "persist serialize: {error}"),
			PersistError::Deserialize(error) => write!(formatter, "persist deserialize: {error}"),
		}
	}
}

impl std::error::Error for PersistError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match self {
			PersistError::Io(error) => Some(error),
			PersistError::Serialize(error) => Some(error),
			PersistError::Deserialize(error) => Some(error),
			PersistError::NotSupported => None,
		}
	}
}

impl From<std::io::Error> for PersistError {
	fn from(error: std::io::Error) -> Self {
		PersistError::Io(error)
	}
}

/// serialize `value` to RON and write it to `path`.
///
/// on WASM this always returns [`PersistError::NotSupported`].
#[cfg(not(target_arch = "wasm32"))]
pub fn save<T: Serialize>(path: &str, value: &T) -> Result<(), PersistError> {
	let content = ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())
		.map_err(PersistError::Serialize)?;
	if let Some(parent) = std::path::Path::new(path).parent()
		&& !parent.as_os_str().is_empty()
	{
		std::fs::create_dir_all(parent)?;
	}
	std::fs::write(path, content.as_bytes())?;
	Ok(())
}

#[cfg(target_arch = "wasm32")]
pub fn save<T: Serialize>(_path: &str, _value: &T) -> Result<(), PersistError> {
	Err(PersistError::NotSupported)
}

/// read `path` and deserialize it as RON into `T`.
///
/// on WASM this always returns [`PersistError::NotSupported`].
#[cfg(not(target_arch = "wasm32"))]
pub fn load<T: DeserializeOwned>(path: &str) -> Result<T, PersistError> {
	let content = std::fs::read_to_string(path)?;
	ron::from_str(&content).map_err(PersistError::Deserialize)
}

#[cfg(target_arch = "wasm32")]
pub fn load<T: DeserializeOwned>(_path: &str) -> Result<T, PersistError> {
	Err(PersistError::NotSupported)
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde::{Deserialize, Serialize};

	#[derive(Debug, PartialEq, Serialize, Deserialize)]
	struct TestData {
		level: u32,
		score: u64,
		name: String,
	}

	#[test]
	fn round_trip_basic() {
		let tmp = std::env::temp_dir().join("lunar_persist_test_basic.ron");
		let path = tmp.to_str().unwrap();
		let original = TestData {
			level: 5,
			score: 9999,
			name: "hero".into(),
		};
		save(path, &original).expect("save failed");
		let loaded: TestData = load(path).expect("load failed");
		assert_eq!(original, loaded);
		let _ = std::fs::remove_file(path);
	}

	#[test]
	fn round_trip_nested_dir() {
		let tmp = std::env::temp_dir()
			.join("lunar_persist_nested")
			.join("slot0.ron");
		let path = tmp.to_str().unwrap();
		let original = TestData {
			level: 1,
			score: 0,
			name: "new game".into(),
		};
		save(path, &original).expect("save with nested dir failed");
		let loaded: TestData = load(path).expect("load from nested dir failed");
		assert_eq!(original, loaded);
		let _ = std::fs::remove_file(path);
	}

	#[test]
	fn load_missing_file_returns_io_error() {
		let result = load::<TestData>("/tmp/lunar_persist_this_does_not_exist.ron");
		assert!(matches!(result, Err(PersistError::Io(_))));
	}

	#[test]
	fn load_bad_ron_returns_deserialize_error() {
		let tmp = std::env::temp_dir().join("lunar_persist_bad.ron");
		let path = tmp.to_str().unwrap();
		std::fs::write(path, b"not valid ron {{{{").unwrap();
		let result = load::<TestData>(path);
		assert!(matches!(result, Err(PersistError::Deserialize(_))));
		let _ = std::fs::remove_file(path);
	}
}
