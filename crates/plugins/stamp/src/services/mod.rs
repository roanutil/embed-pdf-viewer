//! `services/`: plugin-private services every area is built on (not the
//! kernel): the events, the binaries resource, the asset engine port, the
//! ghost renderer and the document a placing verb acts on.

pub mod asset_engine;
pub mod binaries;
pub mod documents;
pub mod errors;
pub mod events;
pub mod ghosts;
pub mod settings;
