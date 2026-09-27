//! `services/`: plugin-private services every area is built on (not the
//! kernel's): the event hooks, the slice reads and engine doors, authority,
//! the sibling planes and mark resolution.

pub mod authority;
pub mod events;
pub mod marks;
pub mod siblings;
pub mod store;
