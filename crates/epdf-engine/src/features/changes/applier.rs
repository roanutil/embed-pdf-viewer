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

    /// Nothing checked and no one named: taking back a refused change, which
    /// only restores what the change itself wrote.
    pub fn rollback() -> Self {
        Self { user_id: None, identity: IdentityClaims::default(), grants: None }
    }

    /// `checkedAuthority`: a single verb's caller, stamped as it says, with
    /// nothing checked: the host that sent it checked it.
    pub fn checked(identity: IdentityClaims) -> Self {
        Self { user_id: identity.user_id.clone(), identity, grants: None }
    }

    /// `authorizeCapability`.
    pub fn require(&self, capability: DocCapability) -> Result<(), EngineError> {
        match &self.grants {
            Some(grants) => grants.require(capability),
            None => Ok(()),
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
        // `describeProtection`.
        let cause = match &protection.certification {
            Some(certification) => format!(
                "certification signature {} (permission {})",
                certification.signature_index, certification.permission
            ),
            None => format!(
                "an existing signature (declared level '{}')",
                protection
                    .enforced
                    .and_then(|level| serde_json::to_value(level).ok())
                    .and_then(|level| level.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "none declared".to_owned())
            ),
        };
        Err(EngineError::with_message(
            EngineErrorCode::ProtectedDocument,
            format!("the document is signed: {cause} forbids '{capability}'"),
        ))
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

impl<'a> ChangeServices<'a> {
    /// With no fonts and the system's ids: taking back a refused change,
    /// which only removes or restores what the change itself wrote.
    pub fn of_clock(clock: &'a dyn Clock) -> Self {
        Self { clock, ids: &crate::ids::SystemIdSource, fonts: None }
    }
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
    let deletes = ops.iter().any(|op| matches!(op, ChangeOp::AnnotationsDelete { .. }));
    refuse_unrecoverable_delete(session, ops.len() > 1 && deletes)?;
    let ChangeServices { clock, ids, fonts } = services;
    let mut ctx = ChangeContext { session, authority, cancel, clock, ids, fonts };
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
    let deletes = record.steps.iter().any(|step| matches!(step, ReverseStep::AnnotationRemove(_)));
    refuse_unrecoverable_delete(session, record.steps.len() > 1 && deletes)?;
    let ChangeServices { clock, ids, fonts } = services;
    let mut ctx = ChangeContext { session, authority, cancel, clock, ids, fonts };
    let mut collected = Collected::default();
    for step in &record.steps {
        cancel.check()?;
        let done = run_step(&mut ctx, step)?;
        collected.push(&mut ctx, done, space)?;
    }
    Ok(collected.finish(authority))
}

/// A delete on a session that is not a layer captures nothing, so it can't
/// be taken back: a change that deletes and has another op or step after
/// or before it, which could be refused once the delete ran, is refused
/// before anything runs. A delete alone has nothing to take it back from.
fn refuse_unrecoverable_delete(
    session: &DocumentSession,
    could_need_taking_back: bool,
) -> Result<(), EngineError> {
    // re-port (Task 8): 8.29 opens open.fatMem as a layer, which captures
    // what a delete removes; then this guard goes.
    if could_need_taking_back && session.last_object_number()?.is_none() {
        return Err(EngineError::with_message(
            EngineErrorCode::NotImplemented,
            "a change that deletes an annotation beside other writes can't be taken back on \
             this session: it is not a layer, so the delete captures nothing (open.fatMem opens \
             a plain document here, where upstream opens a layer; Task 8.29)",
        ));
    }
    Ok(())
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
        ctx.session.journal(&done.reverse)?;
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
    }
}

/// A reverse step's dispatch (`runStep`).
pub(super) fn run_step(
    ctx: &mut ChangeContext<'_>,
    step: &ReverseStep,
) -> Result<Done, EngineError> {
    match step {
        ReverseStep::MetadataRevert(step) => super::metadata::revert_metadata(ctx, step),
        ReverseStep::AttachmentsRemove(step) => super::attachments::remove_attachment(ctx, step),
        ReverseStep::AttachmentsRestore(step) => super::attachments::restore_attachment(ctx, step),
        ReverseStep::AnnotationRemove(step) => super::annotations::remove_annotation(ctx, step),
        ReverseStep::ObjectsRevert(step) => super::annotations::revert_annotation(ctx, step),
        ReverseStep::AnnotationRestore(step) => super::annotations::restore_annotations(ctx, step),
        ReverseStep::AnnotationReorder(step) => super::annotations::reorder_back(ctx, step),
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
    use super::values_equal;
    use serde_json::json;

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
