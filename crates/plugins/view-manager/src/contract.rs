//! `contract.ts`: panes, tabs, and which document each pane shows. A pane
//! owns a set of documents (its tab strip) and one active document, and a
//! document belongs to exactly one pane: panes partition the open
//! documents.

use std::sync::LazyLock;

use epdf_kernel::Token;
use serde::Serialize;

use crate::controller::ViewManagerCapability;

pub type PaneId = String;

/// `PaneInfo`: the public read-only shape of a pane.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneInfo {
    pub id: PaneId,
    /// Documents in this pane, in tab order.
    pub document_ids: Vec<String>,
    /// The document currently shown in this pane.
    pub active_document_id: Option<String>,
}

/// `PaneEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneEvent {
    pub pane_id: PaneId,
}

/// `PaneFocusChangedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneFocusChangedEvent {
    pub pane_id: Option<PaneId>,
}

/// `DocumentMovedEvent`: `None` is "no pane" (opened, or closed).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMovedEvent {
    pub document_id: String,
    pub from_pane_id: Option<PaneId>,
    pub to_pane_id: Option<PaneId>,
}

pub static VIEW_MANAGER: LazyLock<Token<ViewManagerCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "view-manager",
        "add viewManagerPlugin() from '@embedpdf/plugin-view-manager' to your plugins list",
    )
});
