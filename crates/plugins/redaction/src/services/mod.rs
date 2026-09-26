//! `services/`: the plugin-private services every area is built on (not the
//! kernel). `events.ts` and `siblings.ts` are the members of
//! [`crate::controller::RedactionCapability`] they build; `store.ts` is
//! [`store`].

pub mod events;
pub mod store;
