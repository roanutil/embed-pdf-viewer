//! The view-manager plugin through uniffi: one workspace handle over the
//! panes, one event enum. Every member is synchronous, so every method is
//! one hop to the kernel thread.
//!
//! The capability is workspace-scoped. Its lifetime aborts when the kernel
//! is destroyed, never at a document close, so a closed document only
//! leaves its pane (a `DocumentMoved` event with no destination).

use std::sync::Arc;

use epdf_ffi::{KernelBound, KernelPost, Subscription, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::store::Unsubscribe;

use crate::contract::{DocumentMovedEvent, PaneEvent, PaneFocusChangedEvent, PaneInfo};
use crate::controller::ViewManagerCapability;

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ViewManagerEvent {
    PaneCreated { event: PaneEvent },
    PaneRemoved { event: PaneEvent },
    FocusChanged { event: PaneFocusChangedEvent },
    DocumentMoved { event: DocumentMovedEvent },
}

#[uniffi::export(with_foreign)]
pub trait ViewManagerEventListener: Send + Sync {
    fn on_event(&self, event: ViewManagerEvent);
    /// The viewer was destroyed: no event follows. Called once, and at once
    /// for a subscription made after the destroy.
    fn on_end(&self);
}

/// The capability, while the kernel lives.
fn open(cap: &ViewManagerCapability) -> Result<&ViewManagerCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "view-manager",
            "the viewer this capability belongs to is destroyed",
            None,
            None,
        ));
    }
    Ok(cap)
}

#[derive(uniffi::Object)]
pub struct ViewManagerHandle {
    cap: KernelBound<ViewManagerCapability>,
    post: KernelPost,
}

impl ViewManagerHandle {
    /// On the kernel thread.
    pub fn new(cap: ViewManagerCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(ViewManagerHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl ViewManagerHandle {
    /// Panes in display order (`listPanes`).
    pub async fn panes(&self) -> Result<Vec<PaneInfo>, ViewerError> {
        self.cap
            .call(|c| Ok(open(c)?.list_panes().iter().map(|pane| (**pane).clone()).collect()))
            .await
    }

    pub async fn pane(&self, id: String) -> Result<Option<PaneInfo>, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.get_pane(&id).map(|pane| (*pane).clone()))).await
    }

    pub async fn pane_order(&self) -> Result<Vec<String>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_pane_order().to_vec())).await
    }

    pub async fn focused_pane_id(&self) -> Result<Option<String>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_focused_pane_id())).await
    }

    /// The first pane in display order that holds the document.
    pub async fn pane_of_document(
        &self,
        document_id: String,
    ) -> Result<Option<String>, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.get_pane_of_document(&document_id))).await
    }

    /// A new pane, focused; each document moves in from wherever it was.
    /// Answers the new pane's id.
    pub async fn create_pane(&self, document_ids: Vec<String>) -> Result<String, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.create_pane(&document_ids))).await
    }

    /// Its documents go to `move_documents_to`, else the focused pane when
    /// that is another one, else the last remaining pane. An unknown id is
    /// ignored.
    pub async fn remove_pane(
        &self,
        id: String,
        move_documents_to: Option<String>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.remove_pane(&id, move_documents_to.as_deref());
                Ok(())
            })
            .await
    }

    /// Clamped into the order; an unknown id is ignored.
    pub async fn move_pane(&self, id: String, to_index: u32) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.move_pane(&id, f64::from(to_index));
                Ok(())
            })
            .await
    }

    pub async fn set_focused_pane(&self, id: Option<String>) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.set_focused_pane(id.as_deref());
                Ok(())
            })
            .await
    }

    /// A new pane showing the document, beside `from` (or the focused
    /// pane), focused. Answers the new pane's id.
    pub async fn split_pane(
        &self,
        document_id: String,
        from: Option<String>,
    ) -> Result<String, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.split_pane(&document_id, from.as_deref()))).await
    }

    /// A document the pane doesn't hold is ignored.
    pub async fn set_active_document(
        &self,
        pane_id: String,
        document_id: Option<String>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.set_active_document(&pane_id, document_id.as_deref());
                Ok(())
            })
            .await
    }

    /// A document another pane holds moves. `index` is clamped; `None`
    /// appends.
    pub async fn add_document(
        &self,
        pane_id: String,
        document_id: String,
        index: Option<u32>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.add_document(&pane_id, &document_id, index.map(f64::from));
                Ok(())
            })
            .await
    }

    pub async fn remove_document(
        &self,
        pane_id: String,
        document_id: String,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.remove_document(&pane_id, &document_id);
                Ok(())
            })
            .await
    }

    /// Reorder a tab inside its pane; `to_index` is clamped.
    pub async fn move_document_within(
        &self,
        pane_id: String,
        document_id: String,
        to_index: u32,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.move_document_within(&pane_id, &document_id, Some(f64::from(to_index)));
                Ok(())
            })
            .await
    }

    /// Drag a tab into another pane, where it becomes active and the pane
    /// takes the focus.
    pub async fn move_document_between(
        &self,
        from_pane_id: String,
        to_pane_id: String,
        document_id: String,
        to_index: Option<u32>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.move_document_between(
                    &from_pane_id,
                    &to_pane_id,
                    &document_id,
                    to_index.map(f64::from),
                );
                Ok(())
            })
            .await
    }

    /// Registered on the kernel thread after this returns; a cancel before
    /// then still takes effect. `on_end` comes when the viewer is destroyed
    /// (at once if it already has been).
    pub fn subscribe(&self, listener: Arc<dyn ViewManagerEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let (created, removed, focus, moved) =
                        (listener.clone(), listener.clone(), listener.clone(), listener);
                    let offs = [
                        c.on_pane_created(move |e| {
                            created.on_event(ViewManagerEvent::PaneCreated { event: e.clone() });
                            Ok(())
                        }),
                        c.on_pane_removed(move |e| {
                            removed.on_event(ViewManagerEvent::PaneRemoved { event: e.clone() });
                            Ok(())
                        }),
                        c.on_focus_changed(move |e| {
                            focus.on_event(ViewManagerEvent::FocusChanged { event: e.clone() });
                            Ok(())
                        }),
                        c.on_document_moved(move |e| {
                            moved.on_event(ViewManagerEvent::DocumentMoved { event: e.clone() });
                            Ok(())
                        }),
                    ];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
