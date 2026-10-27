//! Writing actions: a widget's events and a field's scripts.
//!
//! Ports `services/src/features/actions/internal/writeActionTree.ts`,
//! `writeWidgetActions.ts`, `services/src/features/forms/internal/
//! fieldScripts.ts` and the write half of `core/src/dto/PdfAction.ts`
//! (`PdfActionWrite`, `WidgetActionsPatch`, `FieldActionsPatch`,
//! `needsScriptRight`, `writesScripts`, `isFieldScript`, `actionWriteOf`,
//! `widgetActionsOf`, `fieldScriptOf`).
//!
//! An action is written as new indirect action dictionaries, bottom up: each
//! node's `next` first, then the node, chained to them in order. Which
//! actions an event may hold, and who may write them, is the caller's to
//! check first.

use serde::{Deserialize, Serialize};

use super::destination::PdfDestination;
use super::reader::{ActionNode, ActionNodeKind, ActionTargetRef, ActionTree};
use crate::dto::Patch;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::read::base::AnnotationActions;
use crate::features::forms::snapshot::FieldActions;

/// An action to write (`PdfActionWrite`): the read shape of a type a write
/// can make, and the actions that run after it, in order. A `uri`'s `isMap`
/// and a submit's `charSet` are read, never written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(deserialize = "D: Deserialize<'de>"))]
pub struct ActionWrite<D = PdfDestination> {
    #[serde(flatten)]
    pub kind: ActionWriteKind<D>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<Vec<ActionWrite<D>>>,
}

/// The node an [`ActionWrite`] makes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    bound(deserialize = "D: Deserialize<'de>")
)]
pub enum ActionWriteKind<D = PdfDestination> {
    Javascript {
        script: String,
    },
    Goto {
        destination: D,
    },
    Uri {
        uri: String,
    },
    Named {
        name: String,
    },
    Hide {
        targets: Vec<ActionTargetRef>,
        hide: bool,
    },
    ResetForm {
        /// `None` resets every field; `[]` with `exclude` false resets none.
        fields: Option<Vec<ActionTargetRef>>,
        exclude: bool,
    },
    SubmitForm {
        url: String,
        /// `None` submits every field the form exports.
        fields: Option<Vec<ActionTargetRef>>,
        /// The raw `/Flags` word; 0 when left out.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flags: Option<u32>,
    },
}

impl<D> ActionWrite<D> {
    fn next(&self) -> &[ActionWrite<D>] {
        self.next.as_deref().unwrap_or_default()
    }

    /// The action with every destination in it converted, down its `next`.
    pub fn try_map_destinations<E, Err>(
        self,
        convert: &mut impl FnMut(D) -> Result<E, Err>,
    ) -> Result<ActionWrite<E>, Err> {
        let next = self
            .next
            .map(|next| {
                next.into_iter()
                    .map(|child| child.try_map_destinations(convert))
                    .collect::<Result<Vec<_>, Err>>()
            })
            .transpose()?;
        let kind = match self.kind {
            ActionWriteKind::Goto { destination } => {
                ActionWriteKind::Goto { destination: convert(destination)? }
            }
            ActionWriteKind::Javascript { script } => ActionWriteKind::Javascript { script },
            ActionWriteKind::Uri { uri } => ActionWriteKind::Uri { uri },
            ActionWriteKind::Named { name } => ActionWriteKind::Named { name },
            ActionWriteKind::Hide { targets, hide } => ActionWriteKind::Hide { targets, hide },
            ActionWriteKind::ResetForm { fields, exclude } => {
                ActionWriteKind::ResetForm { fields, exclude }
            }
            ActionWriteKind::SubmitForm { url, fields, flags } => {
                ActionWriteKind::SubmitForm { url, fields, flags }
            }
        };
        Ok(ActionWrite { kind, next })
    }
}

/// `needsScriptRight`: whether writing `action` puts code or a way out of
/// the viewer into the document, which takes `doc.forms.script`: a
/// JavaScript, submit-form or URI action anywhere in its tree.
pub fn needs_script_right<D>(action: &ActionWrite<D>) -> bool {
    matches!(
        action.kind,
        ActionWriteKind::Javascript { .. }
            | ActionWriteKind::SubmitForm { .. }
            | ActionWriteKind::Uri { .. }
    ) || action.next().iter().any(needs_script_right)
}

/// `isFieldScript`: JavaScript all through, what a field event takes.
pub fn is_field_script<D>(action: &ActionWrite<D>) -> bool {
    matches!(action.kind, ActionWriteKind::Javascript { .. })
        && action.next().iter().all(is_field_script)
}

/// The events a widget's actions run on, in `EPDF_ANNOT_ACTION_*` order
/// (`WIDGET_ACTION_EVENTS`).
pub const WIDGET_ACTION_EVENTS: [&str; 11] = [
    "activate",
    "cursorEnter",
    "cursorExit",
    "mouseDown",
    "mouseUp",
    "focus",
    "blur",
    "pageOpen",
    "pageClose",
    "pageVisible",
    "pageInvisible",
];

/// A widget's actions to write, by event (`WidgetActionsPatch`): an action
/// sets the event's, `null` removes it, an event left out keeps what it has.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    deny_unknown_fields,
    bound(deserialize = "D: Deserialize<'de>"),
    default
)]
pub struct WidgetActionsPatch<D = PdfDestination> {
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub activate: Patch<ActionWrite<D>>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub cursor_enter: Patch<ActionWrite<D>>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub cursor_exit: Patch<ActionWrite<D>>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub mouse_down: Patch<ActionWrite<D>>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub mouse_up: Patch<ActionWrite<D>>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub focus: Patch<ActionWrite<D>>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub blur: Patch<ActionWrite<D>>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub page_open: Patch<ActionWrite<D>>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub page_close: Patch<ActionWrite<D>>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub page_visible: Patch<ActionWrite<D>>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub page_invisible: Patch<ActionWrite<D>>,
}

impl<D> Default for WidgetActionsPatch<D> {
    fn default() -> Self {
        Self {
            activate: Patch::Keep,
            cursor_enter: Patch::Keep,
            cursor_exit: Patch::Keep,
            mouse_down: Patch::Keep,
            mouse_up: Patch::Keep,
            focus: Patch::Keep,
            blur: Patch::Keep,
            page_open: Patch::Keep,
            page_close: Patch::Keep,
            page_visible: Patch::Keep,
            page_invisible: Patch::Keep,
        }
    }
}

impl<D> WidgetActionsPatch<D> {
    /// Each event's write, in [`WIDGET_ACTION_EVENTS`] order.
    pub fn events(&self) -> [&Patch<ActionWrite<D>>; 11] {
        [
            &self.activate,
            &self.cursor_enter,
            &self.cursor_exit,
            &self.mouse_down,
            &self.mouse_up,
            &self.focus,
            &self.blur,
            &self.page_open,
            &self.page_close,
            &self.page_visible,
            &self.page_invisible,
        ]
    }

    /// `writesScripts`: an event gets a tree holding a JavaScript,
    /// submit-form or URI action. Removing one never does.
    pub fn writes_scripts(&self) -> bool {
        self.events().iter().any(|event| event.as_set().is_some_and(needs_script_right))
    }

    /// The patch with every destination in it converted.
    pub fn try_map_destinations<E, Err>(
        self,
        convert: &mut impl FnMut(D) -> Result<E, Err>,
    ) -> Result<WidgetActionsPatch<E>, Err> {
        fn map<D, E, Err>(
            patch: Patch<ActionWrite<D>>,
            convert: &mut impl FnMut(D) -> Result<E, Err>,
        ) -> Result<Patch<ActionWrite<E>>, Err> {
            Ok(match patch {
                Patch::Keep => Patch::Keep,
                Patch::Clear => Patch::Clear,
                Patch::Set(action) => Patch::Set(action.try_map_destinations(convert)?),
            })
        }
        Ok(WidgetActionsPatch {
            activate: map(self.activate, convert)?,
            cursor_enter: map(self.cursor_enter, convert)?,
            cursor_exit: map(self.cursor_exit, convert)?,
            mouse_down: map(self.mouse_down, convert)?,
            mouse_up: map(self.mouse_up, convert)?,
            focus: map(self.focus, convert)?,
            blur: map(self.blur, convert)?,
            page_open: map(self.page_open, convert)?,
            page_close: map(self.page_close, convert)?,
            page_visible: map(self.page_visible, convert)?,
            page_invisible: map(self.page_invisible, convert)?,
        })
    }
}

/// A field event's script: JavaScript, with more scripts after it
/// (`FieldScriptWrite`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldScriptWrite {
    #[serde(rename = "type")]
    pub kind: JavascriptTag,
    pub script: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<Vec<FieldScriptWrite>>,
}

/// The one type a field script has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JavascriptTag {
    Javascript,
}

impl FieldScriptWrite {
    /// As an action write.
    pub fn as_action(&self) -> ActionWrite {
        ActionWrite {
            kind: ActionWriteKind::Javascript { script: self.script.clone() },
            next: self.next.as_ref().map(|next| next.iter().map(Self::as_action).collect()),
        }
    }
}

/// The four events a field's scripts run on, in `EPDF_FORM_ACTION_*` order.
pub const FIELD_SCRIPT_EVENTS: [&str; 4] = ["keystroke", "format", "validate", "calculate"];

/// A field's scripts to write, by event (`FieldActionsPatch`): a script sets
/// one, `null` removes it, an event left out keeps what it has.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct FieldActionsPatch {
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub keystroke: Patch<FieldScriptWrite>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub format: Patch<FieldScriptWrite>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub validate: Patch<FieldScriptWrite>,
    #[serde(skip_serializing_if = "Patch::is_keep")]
    pub calculate: Patch<FieldScriptWrite>,
}

impl FieldActionsPatch {
    /// Each event's write, in [`FIELD_SCRIPT_EVENTS`] order.
    pub fn events(&self) -> [&Patch<FieldScriptWrite>; 4] {
        [&self.keystroke, &self.format, &self.validate, &self.calculate]
    }

    /// `writesScripts`: setting any script does; removing one doesn't.
    pub fn writes_scripts(&self) -> bool {
        self.events().iter().any(|event| event.as_set().is_some())
    }
}

/// `actionWriteOf`: the write of a read action tree, what a copy writes.
/// `None` when the tree is incomplete, or holds a type a write can't make.
pub fn action_write_of<D: Clone>(tree: &ActionTree<D>) -> Option<ActionWrite<D>> {
    if tree.incomplete {
        return None;
    }
    node_write_of(tree.root.as_ref()?)
}

fn node_write_of<D: Clone>(node: &ActionNode<D>) -> Option<ActionWrite<D>> {
    let next = node.next.iter().map(node_write_of).collect::<Option<Vec<_>>>()?;
    let kind = match &node.kind {
        ActionNodeKind::Javascript { script } => {
            ActionWriteKind::Javascript { script: script.clone() }
        }
        ActionNodeKind::Goto { destination } => {
            ActionWriteKind::Goto { destination: destination.clone() }
        }
        ActionNodeKind::Uri { uri, .. } => ActionWriteKind::Uri { uri: uri.clone() },
        ActionNodeKind::Named { name } => ActionWriteKind::Named { name: name.clone() },
        ActionNodeKind::Hide { targets, hide } => {
            ActionWriteKind::Hide { targets: targets.clone(), hide: *hide }
        }
        ActionNodeKind::ResetForm { fields, exclude } => {
            ActionWriteKind::ResetForm { fields: fields.clone(), exclude: *exclude }
        }
        ActionNodeKind::SubmitForm { payload: Some(payload) } => ActionWriteKind::SubmitForm {
            url: payload.url.clone(),
            fields: payload.fields.clone(),
            flags: Some(payload.flags.raw),
        },
        _ => return None,
    };
    Some(ActionWrite { kind, next: (!next.is_empty()).then_some(next) })
}

/// `widgetActionsOf`: a widget's read actions as a patch writes them:
/// `null` for an event without one. An event whose actions a write can't
/// make is left out, so writing the patch back never removes it.
pub fn widget_actions_of<D: Clone>(
    actions: Option<&AnnotationActions<D>>,
) -> WidgetActionsPatch<D> {
    let of = |tree: Option<&ActionTree<D>>| match tree {
        None => Patch::Clear,
        Some(tree) => action_write_of(tree).map_or(Patch::Keep, Patch::Set),
    };
    let read =
        |pick: fn(&AnnotationActions<D>) -> Option<&ActionTree<D>>| of(actions.and_then(pick));
    WidgetActionsPatch {
        activate: read(|a| a.activate.as_ref()),
        cursor_enter: read(|a| a.cursor_enter.as_ref()),
        cursor_exit: read(|a| a.cursor_exit.as_ref()),
        mouse_down: read(|a| a.mouse_down.as_ref()),
        mouse_up: read(|a| a.mouse_up.as_ref()),
        focus: read(|a| a.focus.as_ref()),
        blur: read(|a| a.blur.as_ref()),
        page_open: read(|a| a.page_open.as_ref()),
        page_close: read(|a| a.page_close.as_ref()),
        page_visible: read(|a| a.page_visible.as_ref()),
        page_invisible: read(|a| a.page_invisible.as_ref()),
    }
}

/// `fieldScriptOf`: a read tree as the script that writes it, when it is
/// JavaScript all through.
pub fn field_script_of<D: Clone>(tree: &ActionTree<D>) -> Option<FieldScriptWrite> {
    fn script_of<D>(write: &ActionWrite<D>) -> Option<FieldScriptWrite> {
        let ActionWriteKind::Javascript { script } = &write.kind else { return None };
        let next = match &write.next {
            None => None,
            Some(next) => Some(next.iter().map(script_of).collect::<Option<Vec<_>>>()?),
        };
        Some(FieldScriptWrite { kind: JavascriptTag::Javascript, script: script.clone(), next })
    }
    script_of(&action_write_of(tree)?)
}

/// `scriptsOf`: a field's scripts as a patch writes them: `null` for an
/// event without one; an event whose actions a write can't make is left
/// out, so putting the patch back never removes it.
pub fn scripts_of<D: Clone>(actions: Option<&FieldActions<D>>) -> FieldActionsPatch {
    let of = |tree: Option<&ActionTree<D>>| match tree {
        None => Patch::Clear,
        Some(tree) => field_script_of(tree).map_or(Patch::Keep, Patch::Set),
    };
    FieldActionsPatch {
        keystroke: of(actions.and_then(|a| a.keystroke.as_ref())),
        format: of(actions.and_then(|a| a.format.as_ref())),
        validate: of(actions.and_then(|a| a.validate.as_ref())),
        calculate: of(actions.and_then(|a| a.calculate.as_ref())),
    }
}

/// `writeActionTree`: `action` as new indirect action dictionaries of the
/// document, bottom up, the root's handle answered.
pub fn write_action_tree(
    document: &pdfium::Document,
    action: &ActionWrite,
) -> Result<pdfium::Action, EngineError> {
    let next = action
        .next()
        .iter()
        .map(|child| write_action_tree(document, child))
        .collect::<Result<Vec<_>, _>>()?;
    let node = create_action(document, action)?;
    if !next.is_empty() && !document.set_action_next(node, &next) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("the {} action's next ones could not be set", type_name(&action.kind)),
        ));
    }
    Ok(node)
}

fn type_name<D>(kind: &ActionWriteKind<D>) -> &'static str {
    match kind {
        ActionWriteKind::Javascript { .. } => "javascript",
        ActionWriteKind::Goto { .. } => "goto",
        ActionWriteKind::Uri { .. } => "uri",
        ActionWriteKind::Named { .. } => "named",
        ActionWriteKind::Hide { .. } => "hide",
        ActionWriteKind::ResetForm { .. } => "reset-form",
        ActionWriteKind::SubmitForm { .. } => "submit-form",
    }
}

fn targets_of(targets: &[ActionTargetRef]) -> Vec<pdfium::ActionTarget> {
    targets
        .iter()
        .map(|target| match target {
            ActionTargetRef::Name { name } => pdfium::ActionTarget::Name(name.clone()),
            ActionTargetRef::ObjectNumber { object_number } => {
                pdfium::ActionTarget::ObjectNumber(*object_number)
            }
        })
        .collect()
}

/// One action dictionary, without its `next` (`createAction`).
fn create_action(
    document: &pdfium::Document,
    action: &ActionWrite,
) -> Result<pdfium::Action, EngineError> {
    let made = match &action.kind {
        ActionWriteKind::Javascript { script } => document.create_javascript_action(script),
        ActionWriteKind::Goto { destination } => {
            document.create_goto_action(build_destination(document, destination)?)
        }
        ActionWriteKind::Uri { uri } => document.create_uri_action(uri),
        ActionWriteKind::Named { name } => document.create_named_action(name),
        ActionWriteKind::Hide { targets, hide } => {
            document.create_hide_action(&targets_of(targets), *hide)
        }
        ActionWriteKind::ResetForm { fields, exclude } => {
            let targets = fields.as_deref().map(targets_of);
            document.create_reset_form_action(targets.as_deref(), *exclude)
        }
        ActionWriteKind::SubmitForm { url, fields, flags } => {
            let targets = fields.as_deref().map(targets_of);
            document.create_submit_form_action(url, targets.as_deref(), flags.unwrap_or(0))
        }
    };
    made.map_err(|_| {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("the {} action could not be written", type_name(&action.kind)),
        )
    })
}

/// `writeWidgetActions`: set a widget's actions as `patch` says. `None`
/// removes every event. On a field merged with its widget, the field's own
/// events are left alone (the runtime writes only the widget's keys).
pub fn write_widget_actions(
    document: &pdfium::Document,
    annotation: &pdfium::Annotation<'_>,
    patch: Option<&WidgetActionsPatch>,
) -> Result<(), EngineError> {
    let clear = Patch::Clear;
    let events: [&Patch<ActionWrite>; 11] = match patch {
        Some(patch) => patch.events(),
        None => [&clear; 11],
    };
    for (code, (event, write)) in WIDGET_ACTION_EVENTS.iter().zip(events).enumerate() {
        let action = match write {
            Patch::Keep => continue,
            Patch::Clear => None,
            Patch::Set(action) => Some(write_action_tree(document, action)?),
        };
        if !annotation.set_event_action(code as i32, action) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("the widget's {event} action could not be written"),
            ));
        }
    }
    Ok(())
}

/// `writeFieldScripts`: write a field's scripts as `patch` says. An event
/// whose script is already the one written is left alone, so a write that
/// changes nothing copies nothing up for writing.
pub fn write_field_scripts(
    document: &pdfium::Document,
    field_object_number: u32,
    current: Option<&FieldActions>,
    patch: &FieldActionsPatch,
) -> Result<(), EngineError> {
    let trees = [
        current.and_then(|c| c.keystroke.as_ref()),
        current.and_then(|c| c.format.as_ref()),
        current.and_then(|c| c.validate.as_ref()),
        current.and_then(|c| c.calculate.as_ref()),
    ];
    for (code, ((event, write), now)) in
        FIELD_SCRIPT_EVENTS.iter().zip(patch.events()).zip(trees).enumerate()
    {
        let action = match write {
            Patch::Keep => continue,
            Patch::Clear if now.is_none() => continue,
            Patch::Clear => None,
            Patch::Set(script) if now.is_some_and(|tree| holds(tree, script)) => continue,
            Patch::Set(script) => Some(write_action_tree(document, &script.as_action())?),
        };
        if !document.set_field_event_action(field_object_number, code as i32, action) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("the field's {event} script could not be written"),
            ));
        }
    }
    Ok(())
}

/// Whether a read tree is exactly `script`: the same scripts, chained the
/// same way.
fn holds(tree: &ActionTree, script: &FieldScriptWrite) -> bool {
    fn same(node: &ActionNode, write: &FieldScriptWrite) -> bool {
        let ActionNodeKind::Javascript { script } = &node.kind else { return false };
        let next = write.next.as_deref().unwrap_or_default();
        *script == write.script
            && node.next.len() == next.len()
            && node.next.iter().zip(next).all(|(child, write)| same(child, write))
    }
    !tree.incomplete && tree.root.as_ref().is_some_and(|root| same(root, script))
}

/// A field's `/AA` script for `event` is JavaScript only (`isFieldScript`
/// on the write). The wire type already takes only JavaScript, so a script
/// here always is one; kept for a caller building one from an action.
pub fn assert_field_script(event: &str, action: &ActionWrite) -> Result<(), EngineError> {
    if is_field_script(action) {
        return Ok(());
    }
    Err(EngineError::with_message(
        EngineErrorCode::InvalidArg,
        format!("a field's {event} event takes JavaScript only"),
    )
    .with_details(serde_json::json!({ "field": format!("actions.{event}") })))
}

/// Turn a destination into the array PDFium builds.
///
/// `/XYZ` has its own constructor because its three components are each
/// present-or-retain, and a parameter array has nowhere to put that
/// distinction: a missing `top` would arrive as a zero, which is a real
/// coordinate at the bottom of the page.
///
/// The other fit types go through the array, where a missing optional becomes
/// a SHORTER array rather than a zero: the parameters are positional, so
/// that is the only way for one to be absent. PDFium then pads it back.
pub(crate) fn build_destination(
    document: &pdfium::Document,
    destination: &PdfDestination,
) -> Result<pdfium::Destination, EngineError> {
    use pdfium::view;

    let page = destination.page_object_number().get();
    let optional = |value: Option<f64>| value.map(|v| vec![v as f32]).unwrap_or_default();
    let built = match destination {
        PdfDestination::Xyz { left, top, zoom, .. } => document.create_xyz_destination(
            page,
            left.map(|v| v as f32),
            top.map(|v| v as f32),
            zoom.map(|v| v as f32),
        ),
        PdfDestination::Fit { .. } => document.create_view_destination(page, view::FIT, &[]),
        PdfDestination::FitB { .. } => document.create_view_destination(page, view::FIT_B, &[]),
        PdfDestination::FitH { top, .. } => {
            document.create_view_destination(page, view::FIT_H, &optional(*top))
        }
        PdfDestination::FitBh { top, .. } => {
            document.create_view_destination(page, view::FIT_BH, &optional(*top))
        }
        PdfDestination::FitV { left, .. } => {
            document.create_view_destination(page, view::FIT_V, &optional(*left))
        }
        PdfDestination::FitBv { left, .. } => {
            document.create_view_destination(page, view::FIT_BV, &optional(*left))
        }
        PdfDestination::FitR { left, bottom, right, top, .. } => document.create_view_destination(
            page,
            view::FIT_R,
            &[*left as f32, *bottom as f32, *right as f32, *top as f32],
        ),
    };
    built.map_err(|e| {
        EngineError::with_message(
            match e {
                pdfium::PdfiumError::PageLoadByObjectNumber(_) => EngineErrorCode::NotFound,
                _ => EngineErrorCode::Unknown,
            },
            format!("the link destination could not be built: {e}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_action_write_reads_as_the_wire_spells_it() {
        let wire = serde_json::json!({
            "type": "reset-form", "fields": null, "exclude": false,
            "next": [{ "type": "javascript", "script": "app.alert(1)" }],
        });
        let action: ActionWrite = serde_json::from_value(wire.clone()).unwrap();
        assert!(matches!(action.kind, ActionWriteKind::ResetForm { fields: None, exclude: false }));
        assert!(needs_script_right(&action), "a script down the chain takes the right");
        assert!(!is_field_script(&action));
        assert_eq!(serde_json::to_value(&action).unwrap(), wire);
    }

    #[test]
    fn a_widget_patch_tells_keep_from_clear() {
        let patch: WidgetActionsPatch = serde_json::from_value(serde_json::json!({
            "activate": { "type": "named", "name": "NextPage" }, "focus": null,
        }))
        .unwrap();
        assert!(matches!(patch.activate, Patch::Set(_)));
        assert!(patch.focus.is_clear());
        assert!(patch.blur.is_keep());
        assert!(!patch.writes_scripts());
        let unknown =
            serde_json::from_value::<WidgetActionsPatch>(serde_json::json!({ "click": null }));
        assert!(unknown.is_err(), "an event no widget has is refused");
    }

    #[test]
    fn a_field_script_is_javascript_only() {
        let patch: FieldActionsPatch = serde_json::from_value(serde_json::json!({
            "calculate": { "type": "javascript", "script": "event.value = 1" }, "format": null,
        }))
        .unwrap();
        assert!(patch.writes_scripts());
        let foreign = serde_json::from_value::<FieldActionsPatch>(serde_json::json!({
            "calculate": { "type": "named", "name": "NextPage" },
        }));
        assert!(foreign.is_err());
    }
}
