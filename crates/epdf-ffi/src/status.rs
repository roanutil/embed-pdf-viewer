//! `ResourceStatus`, which every plugin that hydrates a resource from the
//! engine reports (metadata, form, signature). Each plugin crate keeps its
//! own serde enum and converts into this one, so the foreign side sees one
//! type rather than one per plugin.

/// Load state of a resource a plugin reads from the engine. `Forbidden` is
/// a permission refusal, not an empty value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ResourceStatus {
    Idle,
    Loading,
    Ready,
    Forbidden,
    Error,
}
