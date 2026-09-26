//! `services/`: plugin-private services every area is built on (not the
//! kernel). The TypeScript builds `{events, store, siblings}` once; here
//! they are the fields of [`Services`], and `store`'s reads are its methods.

pub mod events;
pub mod store;

pub use events::MeasurementEvents;
pub use store::{PageMeta, Services};
