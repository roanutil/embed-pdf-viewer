//! `write/drafts.ts`: creation drafts. The live multi-click or buffered-ink
//! draft a gesture builds, its commit and cancel doors, and the
//! captured-draft seam the measurement plugin calibrates from.

use epdf_annotation_core::{
    AnnotationRef, CreationDraftAnchor, Draft, Geom, Msg, PageRef, content_to_pdf_point,
    creation_draft_anchor,
};
use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, Unsubscribe};

use crate::controller::AnnotationCapability;
use crate::host_contract::CapturedAnnotationDraft;
use crate::services::write_sink::create_effects_of;

fn is_create_draft(d: &Draft) -> bool {
    serde_json::to_value(d)
        .ok()
        .and_then(|v| v.get("g").and_then(|g| g.as_str()).map(|g| g.starts_with("create-")))
        .unwrap_or(false)
}

impl AnnotationCapability {
    pub(crate) fn run_captured_effect(&self, tool: String, page: PageRef, geom: Geom) {
        if self.doc().is_none() {
            return;
        }
        let Some(crop) = self.crop_of(page.page_object_number) else { return };
        if let Geom::Line { a, b, .. } = geom {
            let pdf = |p| {
                let q = content_to_pdf_point(p, crop);
                epdf_annotation_core::Point { x: q.x, y: q.y }
            };
            self.0.events.draft_captured.emit(&CapturedAnnotationDraft {
                tool,
                page,
                from: pdf(a),
                to: pdf(b),
            });
        }
    }

    /// `getCreationDraft()`: the live multi-click draft, or `None`.
    pub fn get_creation_draft(&self) -> Option<CreationDraftAnchor> {
        creation_draft_anchor(&self.model())
    }

    /// `hasCreationDraft()`.
    pub fn has_creation_draft(&self) -> bool {
        self.model().draft.as_ref().is_some_and(is_create_draft)
    }

    /// `finishCreationDraft()`: commit the draft; `None` when there was none.
    pub fn finish_creation_draft(&self) -> Promise<Option<AnnotationRef>> {
        let draft = self.model().draft.clone();
        let Some(draft) = draft.filter(is_create_draft) else { return Promise::resolved(None) };
        let msg = if matches!(draft, Draft::CreateInk { .. }) {
            Msg::FinishInkDraft
        } else {
            Msg::FinishCreationDraft
        };
        let effects = self.commit(msg);
        match create_effects_of(&effects).into_iter().next() {
            Some(id) => {
                let created = self.await_create(Some(id));
                // An async function returning a promise adopts it.
                Promise::spawn(async move {
                    epdf_kernel::rt::tick().await;
                    created.await.map(Some)
                })
            }
            None => Promise::resolved(None),
        }
    }

    /// `finishInkDraft()` (host).
    pub fn finish_ink_draft(&self) {
        self.commit(Msg::FinishInkDraft);
    }

    /// `cancelCreationDraft()`.
    pub fn cancel_creation_draft(&self) {
        self.commit(Msg::Cancel);
    }

    /// `cancel()`: the in-flight gesture or draft.
    pub fn cancel(&self) {
        self.commit(Msg::Cancel);
    }

    /// `distanceCreationPage()` (host): the page of a distance draft placing
    /// its offset.
    pub fn distance_creation_page(&self) -> Option<PageRef> {
        match &self.model().draft {
            Some(Draft::CreateDistance { step, page, .. })
                if *step == epdf_annotation_core::DistanceStep::Offset =>
            {
                Some(*page)
            }
            _ => None,
        }
    }

    /// `onDraftCaptured(listener)` (host).
    pub fn on_draft_captured(
        &self,
        listener: impl Fn(&CapturedAnnotationDraft) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.draft_captured.on(listener)
    }
}
