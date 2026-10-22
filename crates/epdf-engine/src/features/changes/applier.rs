//! Running a change's ops, or the steps of a recorded reverse
//! (`services/src/features/changes/ChangeApplier.ts`).
//!
//! Each op and step is checked against the change's authority as it runs,
//! as its single verb checks it. Every op records the steps that reverse it
//! as it runs, from what it found and wrote; the record lists them in
//! reverse op order, so each step meets the document as the op after it
//! left it. Every op's reverse also goes into the running transaction's
//! journal, which is what takes a refused change back
//! ([`super::transaction`]).

use super::item::ChangeItem;
use super::op::ChangeOp;
use super::record::{ChangeRecord, ReverseStep};
use crate::auth::scope::{DocCapability, IdentityClaims};
use crate::cancel::CancelToken;
use crate::clock::Clock;
use crate::error::{EngineError, EngineErrorCode};
use crate::ids::IdSource;
use crate::mutation::MutationMeta;
use crate::session::authority::SessionAuthority;
use crate::session::document::DocumentSession;

/// Who a change acts for and what it may do (`ChangeAuthority`).
#[derive(Debug, Clone)]
pub struct ChangeAuthority {
    /// Who makes the change: what its record names, and who may undo it.
    pub user_id: Option<String>,
    /// Who the change acts for, as a create stamps it (`identity`).
    pub identity: IdentityClaims,
    /// What each op is checked against. `None`: nothing is checked (a
    /// tenant writing its own document, `grants: null`).
    pub grants: Option<SessionAuthority>,
}

impl ChangeAuthority {
    /// The session's own authority: a write whose request named none.
    pub fn of_session(session: &DocumentSession) -> Self {
        let identity = session.authority().identity().clone();
        Self {
            user_id: identity.user_id.clone(),
            identity,
            grants: Some(session.authority().clone()),
        }
    }

    /// `checkedAuthority`: a single verb's caller, stamped as it says, with
    /// nothing checked: the host that sent it checked it.
    pub fn checked(identity: IdentityClaims) -> Self {
        Self { user_id: identity.user_id.clone(), identity, grants: None }
    }

    /// `authorizeCapability`: refused `PermissionDenied(capability,
    /// 'target')`.
    pub fn require(&self, capability: DocCapability) -> Result<(), EngineError> {
        match &self.grants {
            Some(grants) if !grants.allows(capability) => {
                Err(EngineError::permission_denied(capability.as_str(), Some("target"), None))
            }
            _ => Ok(()),
        }
    }

    /// `authorizeUnprotected`: what the document's signatures forbid is
    /// refused `ProtectedDocument`, whoever asks.
    pub fn require_unprotected(&self, capability: DocCapability) -> Result<(), EngineError> {
        let Some(protection) = self.grants.as_ref().and_then(SessionAuthority::protection) else {
            return Ok(());
        };
        if !crate::features::signature::protection::protected_capabilities(Some(protection))
            .contains(&capability)
        {
            return Ok(());
        }
        Err(EngineError::with_message(
            EngineErrorCode::ProtectedDocument,
            crate::features::signature::protection::describe_protection(capability, protection),
        ))
    }
}

impl ChangeAuthority {
    /// `allowsFieldWrite`: whether the caller may fill in or sign the field
    /// in `group_id` (`None`: in no group). Nothing checked, always.
    pub fn allows_field_write(
        &self,
        action: crate::auth::scope::FieldWriteAction,
        group_id: Option<&str>,
    ) -> bool {
        self.grants.as_ref().is_none_or(|grants| grants.allows_field_write(action, group_id))
    }

    /// `allowsSomeFieldWrite`: whether the caller may `action` some field,
    /// before the write knows which.
    pub fn allows_some_field_write(&self, action: crate::auth::scope::FieldWriteAction) -> bool {
        self.grants.as_ref().is_none_or(|grants| grants.allows_some_field_write(action))
    }

    /// `authorizeFieldWrite`: what the signatures forbid is
    /// `ProtectedDocument`, and a field outside the caller's groups
    /// `Forbidden`, naming the field scope it takes.
    pub fn require_field_write(
        &self,
        group_id: Option<&str>,
        action: crate::auth::scope::FieldWriteAction,
    ) -> Result<(), EngineError> {
        self.require_unprotected(action.broad())?;
        if self.allows_field_write(action, group_id) {
            return Ok(());
        }
        let verb = match action {
            crate::auth::scope::FieldWriteAction::Fill => "fill",
            crate::auth::scope::FieldWriteAction::Sign => "sign",
        };
        let needs = match group_id {
            None => action.broad().to_string(),
            Some(group) => format!("fields:{verb}:group={group}"),
        };
        Err(EngineError::permission_denied(&needs, Some("target"), None))
    }
}

impl ChangeAuthority {
    /// `authorizeFieldGroup`: putting a field in `group_id` takes
    /// `fields:set-group` for it, unless it is the caller's own group.
    pub fn require_field_group(&self, group_id: &str) -> Result<(), EngineError> {
        match &self.grants {
            Some(grants) if !grants.allows_field_group(group_id) => {
                Err(EngineError::permission_denied(
                    &format!("fields:set-group:group={group_id}"),
                    Some("target"),
                    None,
                ))
            }
            _ => Ok(()),
        }
    }
}

/// What a change fills in that its caller may fill (`fillsOwnField`), asked
/// only when a field outside the caller's groups is written, and answered
/// once, from the fields as they are then: the fields its value and text
/// ops name (`opsFillOwnField`), or the fields its undo puts back a fill of
/// (`stepsFillOwnField`).
#[derive(Debug, Default)]
pub struct OwnFill {
    pub refs: Vec<crate::features::forms::snapshot::FormFieldRef>,
    pub answer: std::cell::Cell<Option<bool>>,
}

impl OwnFill {
    fn of_ops(ops: &[ChangeOp]) -> Self {
        let refs = ops
            .iter()
            .filter_map(|op| match op {
                ChangeOp::FormsSetValue { field, .. }
                | ChangeOp::FormsSetAppearanceText { field, .. } => Some(field.clone()),
                _ => None,
            })
            .collect();
        Self { refs, answer: std::cell::Cell::new(None) }
    }

    fn of_steps(steps: &[ReverseStep]) -> Self {
        use super::record::RevertSubject;
        let refs = steps
            .iter()
            .filter_map(|step| match step {
                ReverseStep::ObjectsRevert(step)
                    if matches!(
                        step.reports.as_str(),
                        "forms.setValue" | "forms.setAppearanceText"
                    ) =>
                {
                    match &step.subject {
                        RevertSubject::Fields { fields } => Some(fields.clone()),
                        RevertSubject::Annotation { .. } | RevertSubject::OutlineItem { .. } => {
                            None
                        }
                    }
                }
                _ => None,
            })
            .flatten()
            .map(|field| crate::features::forms::snapshot::FormFieldRef::ObjectNumber { field })
            .collect();
        Self { refs, answer: std::cell::Cell::new(None) }
    }
}

/// What a change writes with besides its document and authority: the
/// clock, the id source, and this thread's fonts (a free text in a
/// registered face).
#[derive(Clone, Copy)]
pub struct ChangeServices<'a> {
    pub clock: &'a dyn Clock,
    pub ids: &'a dyn IdSource,
    pub fonts: Option<&'a crate::features::fonts::FontRegistry>,
}

/// What every op and step of a change runs with (`ChangeContext`).
pub struct ChangeContext<'a> {
    pub session: &'a mut DocumentSession,
    pub authority: &'a ChangeAuthority,
    pub cancel: &'a CancelToken,
    /// What a write stamps a date from (an attachment's creation date).
    pub clock: &'a dyn Clock,
    /// Where a fresh name comes from (a created annotation's `/NM`).
    pub ids: &'a dyn IdSource,
    /// This thread's registered fonts, for a free text's face.
    pub fonts: Option<&'a crate::features::fonts::FontRegistry>,
    /// The fields the change fills in, for `fillsOwnField`.
    pub own_fill: OwnFill,
}

/// What running an op or a step did: its item, and the steps that reverse
/// it, in the order they run (`Done`).
#[derive(Debug)]
pub struct Done {
    pub item: ChangeItem,
    pub reverse: Vec<ReverseStep>,
}

/// What a change did: one item per op (or per step of an undo), and the
/// record that undoes it (`AppliedChange`). Its steps are empty when nothing
/// was written.
#[derive(Debug)]
pub struct AppliedChange {
    pub items: Vec<ChangeItem>,
    pub record: ChangeRecord,
}

/// Where a change's places are measured (`ChangeSpace`). Each op's places
/// turn into the file's coordinates right before it runs, and each item's
/// into the answer's space right after, against the document as it is then:
/// an earlier op can add, remove or put back a page a later one names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeSpace {
    /// The file's own coordinates, in and out: a single verb's one-op
    /// change, whose request and answer the host boundary converts
    /// (`FILE_SPACE`).
    File,
    /// Page space both ways: `document.apply` and `document.applyChanges`.
    Page,
}

impl ChangeSpace {
    fn op_in_file_space(
        self,
        session: &DocumentSession,
        op: ChangeOp,
    ) -> Result<ChangeOp, EngineError> {
        match self {
            Self::File => Ok(op),
            Self::Page => crate::page_space::boundary::change_op_in_file_space(op, &mut |page| {
                crate::features::pages::visible_box_of(session.document()?, page)
            }),
        }
    }

    fn item_out(
        self,
        session: &DocumentSession,
        item: ChangeItem,
    ) -> Result<ChangeItem, EngineError> {
        match self {
            Self::File => Ok(item),
            Self::Page => {
                crate::page_space::boundary::change_item_in_page_space(item, &mut |page| {
                    crate::features::pages::visible_box_of(session.document()?, page)
                })
            }
        }
    }
}

/// Run a change's ops in order, inside the transaction the caller opened.
pub fn apply(
    session: &mut DocumentSession,
    ops: Vec<ChangeOp>,
    authority: &ChangeAuthority,
    cancel: &CancelToken,
    services: ChangeServices<'_>,
    space: ChangeSpace,
) -> Result<AppliedChange, EngineError> {
    let ChangeServices { clock, ids, fonts } = services;
    let own_fill = OwnFill::of_ops(&ops);
    let mut ctx = ChangeContext { session, authority, cancel, clock, ids, fonts, own_fill };
    let mut collected = Collected::default();
    for (at, op) in ops.into_iter().enumerate() {
        cancel.check()?;
        let op = space.op_in_file_space(ctx.session, op)?;
        let done = run_op(&mut ctx, &op, at)?;
        collected.push(&mut ctx, done, space)?;
    }
    Ok(collected.finish(authority))
}

/// Run the steps of a recorded reverse, inside the transaction the caller
/// opened.
pub fn undo(
    session: &mut DocumentSession,
    record: &ChangeRecord,
    authority: &ChangeAuthority,
    cancel: &CancelToken,
    services: ChangeServices<'_>,
    space: ChangeSpace,
) -> Result<AppliedChange, EngineError> {
    let ChangeServices { clock, ids, fonts } = services;
    let own_fill = OwnFill::of_steps(&record.steps);
    let mut ctx = ChangeContext { session, authority, cancel, clock, ids, fonts, own_fill };
    let mut collected = Collected::default();
    for step in &record.steps {
        cancel.check()?;
        let done = run_step(&mut ctx, step)?;
        collected.push(&mut ctx, done, space)?;
    }
    Ok(collected.finish(authority))
}

/// Each run's item, measured as soon as it ran, and its reverse
/// (`collect`).
#[derive(Default)]
struct Collected {
    items: Vec<ChangeItem>,
    reverses: Vec<Vec<ReverseStep>>,
}

impl Collected {
    fn push(
        &mut self,
        ctx: &mut ChangeContext<'_>,
        done: Done,
        space: ChangeSpace,
    ) -> Result<(), EngineError> {
        self.items.push(space.item_out(ctx.session, done.item)?);
        self.reverses.push(done.reverse);
        Ok(())
    }

    /// The record reverses the runs in the opposite order.
    fn finish(self, authority: &ChangeAuthority) -> AppliedChange {
        AppliedChange {
            items: self.items,
            record: ChangeRecord {
                user_id: authority.user_id.clone(),
                steps: self.reverses.into_iter().rev().flatten().collect(),
            },
        }
    }
}

/// The op table's dispatch (`runOp`). A variant added to [`ChangeOp`] does
/// not compile until it has an arm here.
fn run_op(ctx: &mut ChangeContext<'_>, op: &ChangeOp, at: usize) -> Result<Done, EngineError> {
    match op {
        ChangeOp::MetadataUpdate { patch, expect, keys } => {
            super::metadata::update_metadata(ctx, patch, expect.as_deref(), keys, at)
        }
        ChangeOp::MetadataUpdateCustom { patch, keys } => {
            super::metadata::update_custom_metadata(ctx, patch, keys)
        }
        ChangeOp::AttachmentsCreate { file, resources } => {
            let bytes = resources.file.as_ref().map(|file| file.bytes()).transpose()?;
            super::attachments::create_attachment(ctx, file, bytes)
        }
        ChangeOp::AttachmentsDelete { reference } => {
            super::attachments::delete_attachment(ctx, reference)
        }
        ChangeOp::AnnotationsCreate { page, data, object_number, resources } => {
            super::annotations::create_annotation(
                ctx,
                *page,
                data,
                object_number.as_ref(),
                resources,
            )
        }
        ChangeOp::AnnotationsImport { bundle, pages, attribution, actor, limits } => {
            super::annotations::import_annotations(
                ctx,
                bundle,
                pages.as_ref(),
                *attribution,
                actor.as_ref(),
                limits,
            )
        }
        ChangeOp::FormsImport { bundle, pages, attribution, values, actor, limits, may_script } => {
            super::fields::import_form(
                ctx,
                bundle,
                pages.as_ref(),
                *attribution,
                values.unwrap_or(true),
                actor.as_ref(),
                limits,
                may_script.unwrap_or(false),
            )
        }
        ChangeOp::FormsImportValues { bundle, attribution, actor, limits } => {
            super::forms::import_form_values(ctx, bundle, *attribution, actor.as_ref(), limits)
        }
        ChangeOp::AnnotationsUpdate { reference, patch, resources, expect } => {
            super::annotations::update_annotation(
                ctx,
                reference,
                patch,
                resources,
                expect.as_ref(),
                at,
            )
        }
        ChangeOp::AnnotationsDelete { reference, expect } => {
            super::annotations::delete_annotation(ctx, reference, expect.as_ref(), at)
        }
        ChangeOp::AnnotationsReorder { page, refs, position } => {
            super::annotations::reorder_annotations(ctx, *page, refs, position)
        }
        ChangeOp::PagesRotate { pages, rotation } => {
            super::pages::rotate_pages(ctx, pages, rotation)
        }
        ChangeOp::PagesReorder { pages, position } => {
            super::pages::reorder_pages(ctx, pages, position)
        }
        ChangeOp::PagesDelete { pages } => super::pages::delete_pages(ctx, pages),
        ChangeOp::PagesInsertBlank { spec, position, object_numbers } => {
            super::pages::insert_blank_pages(
                ctx,
                spec,
                position.as_ref(),
                object_numbers.as_deref(),
            )
        }
        ChangeOp::PagesInsert { source, position } => {
            super::pages::insert_pages(ctx, source.bytes()?, position.as_ref())
        }
        ChangeOp::FormsSetValue { field, value, expect } => {
            super::forms::set_value(ctx, field, value, expect.as_ref(), at)
        }
        ChangeOp::FormsSetDisplay { field, display } => {
            super::forms::set_display(ctx, field, *display)
        }
        ChangeOp::FormsSetAppearanceText { field, text } => {
            super::forms::set_appearance_text(ctx, field, text)
        }
        ChangeOp::FormsReset { fields } => super::forms::reset(ctx, fields.as_deref()),
        ChangeOp::FormsCreate { draft, object_number, widget_object_numbers } => {
            super::fields::create_field(
                ctx,
                draft,
                object_number.as_ref(),
                widget_object_numbers.as_deref(),
            )
        }
        ChangeOp::FormsUpdate { field, patch, expect } => {
            super::fields::update_field(ctx, field, patch, expect.as_ref(), at)
        }
        ChangeOp::FormsDelete { field, expect } => {
            super::fields::delete_field(ctx, field, expect.as_ref(), at)
        }
        ChangeOp::FormsAddWidget { field, placement, object_number, split_object_number } => {
            super::fields::add_widget(
                ctx,
                field,
                placement,
                object_number.as_ref(),
                split_object_number.as_ref(),
            )
        }
        ChangeOp::FormsRemoveWidget { field, widget } => {
            super::fields::remove_widget(ctx, field, widget)
        }
        ChangeOp::FormsDeleteWidget { widget, expect } => {
            super::fields::delete_widget(ctx, widget, expect.as_ref(), at)
        }
        ChangeOp::FormsReorderCalculations { fields, position } => {
            super::fields::reorder_calculations(ctx, fields, position)
        }
        ChangeOp::FormsReorderWidgets { page, widgets, position } => {
            super::fields::reorder_widgets(ctx, *page, widgets, position)
        }
        ChangeOp::FormsSetSignatureAppearance { field, appearance } => {
            super::forms::set_signature_appearance(ctx, field, appearance.pdf.bytes()?)
        }
        ChangeOp::FormsUpdateWidget { widget, patch, expect } => {
            super::fields::update_widget(ctx, widget, patch, expect.as_ref(), at)
        }
        ChangeOp::OutlineCreate { parent, position, data, object_number } => {
            super::outline::create_outline_item(
                ctx,
                *parent,
                position.as_ref(),
                data,
                object_number.as_ref(),
            )
        }
        ChangeOp::OutlineUpdate { reference, patch, expect } => {
            super::outline::update_outline_item(ctx, *reference, patch, expect.as_ref(), at)
        }
        ChangeOp::OutlineMove { refs, parent, position } => {
            super::outline::move_outline_items(ctx, refs, *parent, position)
        }
        ChangeOp::OutlineDelete { reference, expect } => {
            super::outline::delete_outline_item(ctx, *reference, expect.as_ref(), at)
        }
    }
}

/// A reverse step's dispatch (`runStep`).
fn run_step(ctx: &mut ChangeContext<'_>, step: &ReverseStep) -> Result<Done, EngineError> {
    match step {
        ReverseStep::MetadataRevert(step) => super::metadata::revert_metadata(ctx, step),
        ReverseStep::AttachmentsRemove(step) => super::attachments::remove_attachment(ctx, step),
        ReverseStep::AttachmentsRestore(step) => super::attachments::restore_attachment(ctx, step),
        ReverseStep::AnnotationRemove(step) => super::annotations::remove_annotation(ctx, step),
        ReverseStep::ObjectsRevert(step) => match step.subject {
            super::record::RevertSubject::Fields { .. } => super::forms::revert_form(ctx, step),
            super::record::RevertSubject::Annotation { .. } => {
                super::annotations::revert_annotation(ctx, step)
            }
            super::record::RevertSubject::OutlineItem { .. } => {
                super::outline::revert_outline_item(ctx, step)
            }
        },
        ReverseStep::AnnotationRestore(step) => super::annotations::restore_annotations(ctx, step),
        ReverseStep::AnnotationReorder(step) => super::annotations::reorder_back(ctx, step),
        ReverseStep::PagesRotate(step) => super::pages::rotate_pages_back(ctx, step),
        ReverseStep::PagesReorder(step) => super::pages::reorder_pages_back(ctx, step),
        ReverseStep::PagesRestore(step) => super::pages::restore_pages(ctx, step),
        ReverseStep::PagesRemove(step) => super::pages::remove_pages(ctx, step),
        ReverseStep::FieldRemove(step) => super::fields::remove_field(ctx, step),
        ReverseStep::FieldRestore(step) => super::fields::restore_field(ctx, step),
        ReverseStep::WidgetRestore(step) => super::fields::restore_widget(ctx, step),
        ReverseStep::WidgetDelete(step) => super::fields::delete_restored_widget(ctx, step),
        ReverseStep::CalculationsRestore(step) => {
            super::fields::restore_calculation_order(ctx, step)
        }
        ReverseStep::OutlineRemove(step) => super::outline::remove_outline_item(ctx, step),
        ReverseStep::OutlineRestore(step) => super::outline::restore_outline_item(ctx, step),
        ReverseStep::OutlineMove(step) => super::outline::move_outline_items_back(ctx, step),
        ReverseStep::OutlineReopen(step) => super::outline::reopen_outline_item(ctx, step),
    }
}

/// An op or step that wrote nothing: an item left alone, and nothing to
/// reverse (`leftAlone`).
pub fn left_alone(ctx: &ChangeContext<'_>, op: &str) -> Result<Done, EngineError> {
    Ok(Done {
        item: ChangeItem::Skipped {
            op: op.to_owned(),
            meta: MutationMeta::local(Vec::new(), &ctx.session.write_stamp()?),
        },
        reverse: Vec::new(),
    })
}

/// Whether two JSON values are equal by value (`valuesEqual`): numbers by
/// their value whatever their spelling (`1` and `1.0` are one number), with
/// `Object.is`'s `0` and `-0` apart; arrays item by item; objects by the
/// same keys, whatever their order. A missing value is `undefined`, which
/// equals only another missing one.
pub fn values_equal(a: Option<&serde_json::Value>, b: Option<&serde_json::Value>) -> bool {
    use serde_json::Value;
    match (a, b) {
        (None, None) => true,
        (Some(Value::Number(x)), Some(Value::Number(y))) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => x == y && x.is_sign_negative() == y.is_sign_negative(),
            _ => x == y,
        },
        (Some(Value::Array(x)), Some(Value::Array(y))) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| values_equal(Some(x), Some(y)))
        }
        (Some(Value::Object(x)), Some(Value::Object(y))) => {
            x.len() == y.len() && x.iter().all(|(key, value)| values_equal(Some(value), y.get(key)))
        }
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Refuse the change with `ChangeConflict` when the document doesn't hold
/// what op `op_index` expects: each field of `expected` must have that value
/// now (`assertExpected`).
pub fn assert_expected(
    op_index: usize,
    current: &serde_json::Map<String, serde_json::Value>,
    expected: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), EngineError> {
    let fields: Vec<&str> = expected
        .iter()
        .filter(|(key, value)| !values_equal(current.get(key.as_str()), Some(*value)))
        .map(|(key, _)| key.as_str())
        .collect();
    if fields.is_empty() {
        return Ok(());
    }
    Err(EngineError::with_message(
        EngineErrorCode::ChangeConflict,
        format!("op {op_index} expected other values in {}", fields.join(", ")),
    )
    .with_details(serde_json::json!({ "opIndex": op_index, "fields": fields })))
}

#[cfg(test)]
mod tests {
    use super::{ChangeAuthority, values_equal};
    use crate::auth::scope::{DocCapability, FieldWriteAction, IdentityClaims, PdfBits};
    use crate::session::authority::SessionAuthority;
    use serde_json::json;

    fn granting(scope: &[&str]) -> ChangeAuthority {
        let scope: Vec<String> = scope.iter().map(|s| (*s).to_owned()).collect();
        let grants =
            SessionAuthority::new(&scope, IdentityClaims::default(), PdfBits::default()).unwrap();
        ChangeAuthority { user_id: None, identity: IdentityClaims::default(), grants: Some(grants) }
    }

    #[test]
    fn a_refusal_names_what_it_required_as_permission_denied_does() {
        let authority = granting(&["doc.open"]);
        let field =
            authority.require_field_write(Some("buyer"), FieldWriteAction::Fill).unwrap_err();
        assert_eq!(field.message, "permission denied (target): fields:fill:group=buyer");
        assert_eq!(
            field.details,
            Some(json!({ "required": "fields:fill:group=buyer", "context": "target" }))
        );
        let broad = authority.require_field_write(None, FieldWriteAction::Fill).unwrap_err();
        assert_eq!(
            broad.details,
            Some(json!({ "required": "doc.forms.fill", "context": "target" }))
        );
        let capability = authority.require(DocCapability::DocFormsModify).unwrap_err();
        assert_eq!(capability.message, "permission denied (target): doc.forms.modify");
        assert_eq!(
            capability.details,
            Some(json!({ "required": "doc.forms.modify", "context": "target" }))
        );
    }

    #[test]
    fn values_compare_by_value_as_object_is_does() {
        let same = |a: serde_json::Value, b: serde_json::Value| values_equal(Some(&a), Some(&b));
        assert!(same(json!(1), json!(1.0)), "one number, two spellings");
        assert!(!same(json!(0.0), json!(-0.0)), "Object.is keeps 0 and -0 apart");
        assert!(same(json!({ "x": 1, "y": [2.0] }), json!({ "y": [2], "x": 1.0 })));
        assert!(!same(json!({ "x": 1 }), json!({ "x": 1, "y": null })));
        assert!(!same(json!(null), json!(0)));
        assert!(!values_equal(None, Some(&json!(null))), "undefined isn't null");
        assert!(values_equal(None, None));
    }
}
