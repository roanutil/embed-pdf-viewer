//! `services/`: what every area is built on (not the kernel). The diffing
//! dispatch and its change hooks, the host timing seam and the scene model.
//! The placement latch is [`crate::controller::Inner::placement_started`].

pub mod events;
pub mod scene;
pub mod scheduler;
