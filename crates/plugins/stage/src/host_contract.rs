//! `host-contract.ts`: the host lens. What the surface binding, the gesture
//! controller and sibling plugins need beyond the public contract: the
//! viewport and device ratio reports, the gesture bracket, fling, world
//! space, initial placement, refit and the lens id. The TypeScript types it
//! as `StageHostCapability`, a wider type over the same runtime token; here
//! those members sit on [`crate::StageCapability`] with the rest, and the
//! host token is [`crate::STAGE`].

pub use crate::contract::GestureOptions;
