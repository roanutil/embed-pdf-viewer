//! `services/`: plugin-private services every area is built on (not the
//! kernel): the store (on [`crate::FormCapability`] itself), the event
//! hooks, authority, the sibling planes (fields of [`crate::controller::Core`]),
//! the scripting seam and the one serial mutation queue.

pub mod authority;
pub mod events;
pub mod scripting;
