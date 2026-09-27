//! `model.ts`: the signature slice. The last snapshot and verdicts, the
//! sign-here target, a parked two-phase signing, and the busy flag.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contract::SignaturePending;

/// `ResourceStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceStatus {
    Idle,
    Loading,
    Ready,
    Forbidden,
    Error,
}

/// `SignatureState`. The snapshot and the verdicts are the engine's and
/// `epdf-signature-core`'s JSON.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureState {
    pub snapshot: Option<Value>,
    /// `idle` until the first read; `loading` while one is in flight.
    pub status: ResourceStatus,
    pub verdicts: Option<Value>,
    /// The field the user chose to sign next.
    pub target: Option<Value>,
    /// A signing is parked (here or elsewhere): the document is read-only.
    pub pending: Option<SignaturePending>,
    /// A sign or fill call is in flight.
    pub busy: bool,
}

/// `SignatureAction`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SignatureAction {
    #[serde(rename = "SNAPSHOT")]
    Snapshot { snapshot: Option<Value> },
    #[serde(rename = "STATUS")]
    Status { status: ResourceStatus },
    #[serde(rename = "VERDICTS")]
    Verdicts { verdicts: Option<Value> },
    #[serde(rename = "TARGET")]
    Target { field: Option<Value> },
    #[serde(rename = "PENDING")]
    Pending { pending: Option<SignaturePending> },
    #[serde(rename = "BUSY")]
    Busy { busy: bool },
}

/// `initialSignatureState()`.
pub fn initial_signature_state() -> SignatureState {
    SignatureState {
        snapshot: None,
        status: ResourceStatus::Idle,
        verdicts: None,
        target: None,
        pending: None,
        busy: false,
    }
}

/// `signatureReducer(state, action)`. `None` is the TypeScript's
/// `return state`.
pub fn signature_reducer(
    state: &SignatureState,
    action: &SignatureAction,
) -> Option<SignatureState> {
    let mut next = state.clone();
    match action {
        SignatureAction::Snapshot { snapshot } => next.snapshot = snapshot.clone(),
        SignatureAction::Status { status } => {
            if state.status == *status {
                return None;
            }
            next.status = *status;
        }
        SignatureAction::Verdicts { verdicts } => next.verdicts = verdicts.clone(),
        SignatureAction::Target { field } => next.target = field.clone(),
        SignatureAction::Pending { pending } => next.pending = pending.clone(),
        SignatureAction::Busy { busy } => next.busy = *busy,
    }
    Some(next)
}
