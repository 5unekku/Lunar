//! error handling for the engine
//!
//! provides a unified error type and event system so game code
//! can catch and respond to engine-level failures.

use std::fmt;

/// engine error enum covering common failure modes.
#[derive(Debug, Clone)]
pub enum EngineError {
	/// failed to create the game window
	WindowCreation(String),
	/// failed to initialize the GPU (wgpu)
	GpuInit(String),
	/// failed to load an asset
	AssetLoad { path: String, reason: String },
	/// a handle is invalid or expired
	InvalidHandle(String),
	/// a named scene was not found
	SceneNotFound(String),
	/// a command failed to execute
	Command { name: String, reason: String },
	/// a generic error with a message
	Other(String),
}

impl fmt::Display for EngineError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::WindowCreation(msg) => write!(f, "window creation failed: {msg}"),
			Self::GpuInit(msg) => write!(f, "GPU initialization failed: {msg}"),
			Self::AssetLoad { path, reason } => {
				write!(f, "failed to load asset '{path}': {reason}")
			}
			Self::InvalidHandle(msg) => write!(f, "invalid handle: {msg}"),
			Self::SceneNotFound(name) => write!(f, "scene not found: '{name}'"),
			Self::Command { name, reason } => {
				write!(f, "command '{name}' failed: {reason}")
			}
			Self::Other(msg) => write!(f, "error: {msg}"),
		}
	}
}

impl std::error::Error for EngineError {}

/// convenience result type for engine operations
pub type EngineResult<T> = Result<T, EngineError>;

/// source of an error event
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSource {
	/// error from the render subsystem
	Render,
	/// error from the input subsystem
	Input,
	/// error from the asset subsystem
	Asset,
	/// error from the audio subsystem
	Audio,
	/// error from the core engine
	Core,
	/// error from game code
	Game,
}

/// an error event that can be read by game systems.
///
/// emitted when a recoverable error occurs. game code can
/// listen for these events and respond accordingly.
#[derive(Clone)]
pub struct ErrorEvent {
	/// which subsystem raised the error
	pub source: ErrorSource,
	/// the error details
	pub error: EngineError,
	/// whether the error was automatically recovered
	pub recovered: bool,
}

impl ErrorEvent {
	/// create a new error event
	#[must_use]
	pub const fn new(source: ErrorSource, error: EngineError) -> Self {
		Self {
			source,
			error,
			recovered: false,
		}
	}

	/// mark the error as recovered
	#[must_use]
	pub const fn recovered(mut self) -> Self {
		self.recovered = true;
		self
	}
}

impl bevy_ecs::event::Event for ErrorEvent {
	type Trigger<'a> = bevy_ecs::event::GlobalTrigger;
}
