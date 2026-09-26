//! `services/`: the plugin-private helpers every area is built on. Nothing
//! here belongs to the kernel; these are annotation-specific wrappers over
//! the controller context.

pub mod announce;
pub mod authority;
pub mod events;
pub mod file_picker;
pub mod geometry;
pub mod records;
pub mod store;
pub mod write_sink;
