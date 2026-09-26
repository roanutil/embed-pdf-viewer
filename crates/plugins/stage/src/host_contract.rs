//! `host-contract.ts`: the host lens. What the surface binding, the gesture
//! controller and sibling plugins need beyond the public contract: the
//! viewport and device ratio reports, the gesture bracket, fling, world
//! space, initial placement, refit and the lens id. The TypeScript types it
//! as `StageHostCapability`, a wider type over the same object; here those
//! members sit on [`crate::StageCapability`] with the rest, and this module
//! holds the types only the host lens uses.

use serde::{Deserialize, Serialize};

/// `beginGesture`'s options.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GestureOptions {
    /// Rubber-band overscroll for this gesture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elastic: Option<bool>,
}
