//! `services/`: plugin-private services every area is built on (not the
//! kernel): the event hooks, the binary sidecar, the asset engine port and
//! the ghost renderer.

pub mod asset_engine;
pub mod binaries;
pub mod errors;
pub mod events;
pub mod ghosts;
