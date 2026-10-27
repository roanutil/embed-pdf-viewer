//! Catalog-owned actions.
//!
//! G6. A transcription of
//! `services/src/features/actions/DocumentActionsReader.ts`. Operation:
//! `actions.read`.
//!
//! Extraction only. Nothing here executes an action, and a consumer must not
//! execute a tree whose `incomplete` is set.
//!
//! Page actions stay on their owning page, in
//! [`crate::features::pages::PageLayout`], rather than being collected here:
//! a caller listing pages should not have to make a second call to find out
//! whether opening one runs a script.

use pdfium::{Document, DocumentActionEvent};
use serde::{Deserialize, Serialize};

use super::destination::{PdfDestination, read_destination};
use super::reader::{ActionTree, read_action_model};
use super::vocabulary::ActionReadBudgetTracker;
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};

/// One entry of the JavaScript name tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedJavaScriptAction<D = PdfDestination> {
    /// The name-tree key. Array order is the PDF boot order, which is what
    /// decides which script wins when two define the same function.
    pub name: String,
    pub action: ActionTree<D>,
}

/// Everything the catalog asks for (`DocumentActionsSnapshot<Destination>`).
///
/// The fields are in the order upstream's answer has them: the triggers
/// first, then `nameTreeScripts`, `openAction` and `openDestination`, as
/// `mapDocumentActions` spreads them (60283448c).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound(deserialize = "D: Deserialize<'de>"))]
pub struct DocumentActionsSnapshot<D = PdfDestination> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub will_close: Option<ActionTree<D>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub will_save: Option<ActionTree<D>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did_save: Option<ActionTree<D>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub will_print: Option<ActionTree<D>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did_print: Option<ActionTree<D>>,
    pub name_tree_scripts: Vec<NamedJavaScriptAction<D>>,
    /// `/OpenAction` in its action form. Mutually exclusive with the
    /// destination form, because `/OpenAction` is one entry.
    pub open_action: Option<ActionTree<D>>,
    /// `/OpenAction` in its destination form: the initial view, not an
    /// action. Always written, `null` when there is none.
    #[serde(default)]
    pub open_destination: Option<D>,
}

impl<D> DocumentActionsSnapshot<D> {
    /// The snapshot with every destination in it converted
    /// (`mapDocumentActions`).
    pub fn try_map_destinations<E, Err>(
        self,
        convert: &mut impl FnMut(D) -> Result<E, Err>,
    ) -> Result<DocumentActionsSnapshot<E>, Err> {
        let mut tree = |tree: Option<ActionTree<D>>| {
            tree.map(|tree| tree.try_map_destinations(&mut *convert)).transpose()
        };
        let will_close = tree(self.will_close)?;
        let will_save = tree(self.will_save)?;
        let did_save = tree(self.did_save)?;
        let will_print = tree(self.will_print)?;
        let did_print = tree(self.did_print)?;
        let open_action = tree(self.open_action)?;
        let name_tree_scripts = self
            .name_tree_scripts
            .into_iter()
            .map(|script| {
                Ok(NamedJavaScriptAction {
                    name: script.name,
                    action: script.action.try_map_destinations(&mut *convert)?,
                })
            })
            .collect::<Result<Vec<_>, Err>>()?;
        Ok(DocumentActionsSnapshot {
            will_close,
            will_save,
            did_save,
            will_print,
            did_print,
            name_tree_scripts,
            open_action,
            open_destination: self.open_destination.map(convert).transpose()?,
        })
    }
}

/// Read the catalog's actions.
///
/// One budget across every model, because PDFium's own limits are per model
/// and a catalog with a hundred name-tree scripts could otherwise build an
/// unbounded snapshot out of individually reasonable ones. Destinations are
/// in the file's coordinates; the wire answers them in page space
/// (`crate::page_space::destinations`).
pub fn read_document_actions(
    document: &Document,
    cancel: &CancelToken,
) -> Result<DocumentActionsSnapshot, EngineError> {
    cancel.check()?;
    let mut budget = ActionReadBudgetTracker::default();

    let count = document.name_tree_script_count();
    if count < 0 {
        return Err(EngineError::with_message(
            EngineErrorCode::MalformedPdf,
            "failed to enumerate the PDF name-tree scripts",
        ));
    }

    let mut name_tree_scripts = Vec::with_capacity(count.max(0) as usize);
    for index in 0..count {
        cancel.check()?;
        let name = document
            .name_tree_script_name(index)
            .map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::RuntimeUnavailable,
                    format!("reading name-tree script {index} failed: {e}"),
                )
            })?
            .unwrap_or_default();
        // The key enters the same detached snapshot as a node payload and
        // rides the same budget.
        if !budget.take_payload(name.encode_utf16().count() as u32) {
            return Err(EngineError::with_message(
                EngineErrorCode::MalformedPdf,
                "aggregate PDF action read budget exceeded",
            ));
        }

        // A name-tree entry whose model will not load is an error rather than
        // a skipped script: the document declared a script that runs at open,
        // and quietly dropping it would under-report what the file does.
        let model = document.name_tree_script_model(index).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::MalformedPdf,
                format!("name-tree script {index} has no readable action model"),
            )
        })?;
        let action = read_action_model(&model, &mut budget)?;
        name_tree_scripts.push(NamedJavaScriptAction { name, action });
    }

    cancel.check()?;
    let open_action = match document.open_action_model() {
        Some(model) => Some(read_action_model(&model, &mut budget)?),
        None => None,
    };
    // `/OpenAction` is one entry: the destination form only exists when the
    // action form does not.
    let open_destination = match open_action {
        Some(_) => None,
        None => document.open_action_destination().as_ref().and_then(read_destination),
    };

    let mut read_event = |event: DocumentActionEvent| -> Result<Option<ActionTree>, EngineError> {
        cancel.check()?;
        match document.document_action_model(event) {
            Some(model) => read_action_model(&model, &mut budget).map(Some),
            None => Ok(None),
        }
    };

    Ok(DocumentActionsSnapshot {
        will_close: read_event(DocumentActionEvent::WillClose)?,
        will_save: read_event(DocumentActionEvent::WillSave)?,
        did_save: read_event(DocumentActionEvent::DidSave)?,
        will_print: read_event(DocumentActionEvent::WillPrint)?,
        did_print: read_event(DocumentActionEvent::DidPrint)?,
        name_tree_scripts,
        open_action,
        open_destination,
    })
}
