//! `services/`: plugin-private services every area is built on (not the
//! kernel): the events, the busy bracket and engine doors, authority, the
//! sibling plugins and mark resolution, plus the settings store's port
//! table.

pub mod authority;
pub mod errors;
pub mod events;
pub mod marks;
pub mod settings;
pub mod siblings;
pub mod store;
