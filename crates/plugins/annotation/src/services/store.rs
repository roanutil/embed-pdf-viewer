//! `services/store.ts`: the two doors every change goes through, and one
//! path behind them.
//!
//! `commit(message)` runs a gesture or a selection verb through the core's
//! `update` and builds its effects into the engine's ops; `apply(changes)`
//! states changes in code, each checked by the engine's own rules. Either
//! way the ops go to `ctx.changes.stage(...)`: the view shows them at once
//! (the records mirror predicts them) and the queue sends them after
//! everything staged before. A change leaves the view once the engine
//! answered. New records are named by the object numbers they take, so a
//! record keeps its key from its first frame (H3: no temp id to rename).
//!
//! Op builders are not registered at run time as in the TypeScript (one per
//! effect kind, registered once): [`Core::build_effect`] calls the area that
//! owns each kind directly.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use epdf_annotation_core::types::{Effect, Id, Message, Model, UpdateResult};
use epdf_annotation_core::update::{new_records_at_most, update};
use epdf_kernel::changes::{ChangeQueue, HeldChange, PendingChange, SettledChange, StagedChange};
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::rt::Promise;
use epdf_kernel::{Controller, Error};
use indexmap::IndexMap;
use serde_json::{Value, json};

use crate::controller::{CAPABILITY, Core};
use crate::js::key_or_none;
use crate::model::{AnnotationState, with_object_numbers, with_session, without_object_numbers};
use crate::services::staging::{
    StoreChange, checked_op_of, ids_of, label_of, model_after, model_over, without_covered_deletes,
};
use crate::sync::records::{core_error, predict_records, undo_of};

/// How many changes keep their selections.
const KEPT: usize = 200;

/// How a change the store staged settled. Never a rejection: a refusal is data.
#[derive(Clone, Debug, Default)]
pub struct Written {
    /// The engine's refusal, with the records the change named.
    pub failed: Vec<(Vec<Id>, Error)>,
    /// What the engine answered; `None` when it was refused or wrote nothing.
    pub result: Option<Value>,
}

/// What committing a message started.
#[derive(Clone)]
pub struct Commit {
    pub effects: Vec<Effect>,
    /// Settles when the engine answered the message's change.
    pub written: Promise<Written>,
}

/// How stated changes settled, and what the engine wrote for each.
#[derive(Clone, Debug, Default)]
pub struct AppliedOutcome {
    pub written: Written,
    /// The annotation each create or update left, in the order of the changes.
    pub annotations: Vec<Option<Value>>,
}

/// What applying stated changes started.
#[derive(Clone)]
pub struct Applied {
    pub ids: Vec<Id>,
    pub written: Promise<AppliedOutcome>,
}

/// A change still being made, as typing is.
#[derive(Clone)]
pub struct StoreHold {
    pub(crate) held: HeldChange,
    /// The records from before its first commit: what undoing it brings back.
    base: Rc<RefCell<Option<Rc<Value>>>>,
    mine: Rc<RefCell<HashSet<String>>>,
}

impl StoreHold {
    pub fn open(&self) -> bool {
        self.held.open()
    }
    pub fn send(&self) {
        self.held.send();
    }
    pub fn cancel(&self) {
        if let Some(change) = self.held.change() {
            self.mine.borrow_mut().remove(change.op_id());
        }
        self.held.cancel();
    }
}

/// `adjust`: changes the core's result before it shows.
pub type Adjust = Rc<dyn Fn(UpdateResult) -> UpdateResult>;

/// The store's own data.
pub struct StoreState {
    pub(crate) changes: Option<ChangeQueue>,
    /// The changes this plugin staged that the engine hasn't answered.
    mine: Rc<RefCell<HashSet<String>>>,
    /// The selections around each change, by its `opId`, oldest first.
    kept: RefCell<IndexMap<String, (Vec<Id>, Vec<Id>)>>,
    /// The model the selection, draft, editing and hover events last saw.
    announced: RefCell<Option<Rc<Model>>>,
    watching_lost: Cell<bool>,
}

impl StoreState {
    pub fn new(cx: &Controller<AnnotationState, Value>) -> StoreState {
        StoreState {
            changes: cx.changes().ok(),
            mine: Rc::default(),
            kept: RefCell::new(IndexMap::new()),
            announced: RefCell::new(None),
            watching_lost: Cell::new(false),
        }
    }
}

const NOTHING_WRITTEN: fn() -> Written = Written::default;

fn no_numbers() -> Error {
    plugin_error(
        code::NOT_READY,
        CAPABILITY,
        "the document holds no object number for a new annotation right now",
        None,
        None,
    )
}

/// `refsOfIn(model, ids)`: the refs behind a list of model ids.
pub fn refs_of_in(model: &Model, ids: &[Id]) -> Vec<Value> {
    ids.iter()
        .filter_map(|id| model.by_id.get(id).and_then(|r| r.annotation.get("ref").cloned()))
        .collect()
}

fn same_ids(left: &Rc<Vec<Id>>, right: &Rc<Vec<Id>>) -> bool {
    Rc::ptr_eq(left, right) || **left == **right
}

/// The pending change's `writtenOf`: `.then` on its result.
fn written_of(change: &PendingChange, ids: Vec<Id>) -> Promise<Written> {
    let result = change.result();
    Promise::spawn(async move {
        Ok(match result.await {
            Ok(result) => Written { failed: Vec::new(), result: Some(result) },
            Err(error) => {
                Written { failed: vec![(ids, to_plugin_error(CAPABILITY, &error))], result: None }
            }
        })
    })
}

impl Core {
    fn changes(&self) -> Result<&ChangeQueue, Error> {
        self.store.changes.as_ref().ok_or_else(|| Error::new("[annotation] no document changes"))
    }

    /// The store's listeners (`createStore`): refusals, undo selections, the
    /// forget watch and the session events.
    pub(crate) fn wire_store(&self) -> Result<(), Error> {
        let changes = self.changes()?.clone();
        // A refusal of a change this plugin staged is reported.
        let me = self.weak();
        let off = changes.on_settled().on(move |settled| {
            let Some(core) = me.upgrade() else { return Ok(()) };
            let (change, error) = match settled {
                SettledChange::Applied { change, .. } => {
                    core.store.mine.borrow_mut().remove(change.op_id());
                    return Ok(());
                }
                SettledChange::Refused { change, error } => (change, error),
            };
            if !core.store.mine.borrow_mut().remove(change.op_id()) {
                return Ok(());
            }
            if error.is_plugin_error(Some(code::OPERATION_CANCELLED))
                || error.is_plugin_error(Some(code::INSTANCE_CLOSED))
            {
                return Ok(());
            }
            let refs: Vec<Value> = change.shows().iter().flat_map(refs_named_by).collect();
            core.events
                .write_failed
                .emit(&json!({ "refs": refs, "error": error.plugin_error_info() }));
            Ok(())
        });
        self.cx.listen(off);
        // The selection follows undo and redo (`selection-history.ts`).
        let me = self.weak();
        let off = changes.on_staged().on(move |change| {
            let Some(core) = me.upgrade() else { return Ok(()) };
            core.on_staged(change)
        });
        self.cx.listen(off);
        // A record that left the view leaves the session: synchronous forget
        // (H3). `ctx.watch(view.view, ...)`, whose `Object.is` on the memoized
        // view is the identity of its two members here.
        let (reader, me) = (self.weak(), self.weak());
        self.cx.watch(
            move || Ok(reader.upgrade().map(|core| core.annotation_view())),
            move |next, previous| match (me.upgrade(), next, previous) {
                (Some(core), Some(next), Some(previous)) => core.forget_gone(next, previous),
                _ => Ok(()),
            },
            |a, b| match (a, b) {
                (Some(a), Some(b)) => {
                    Rc::ptr_eq(&a.by_id, &b.by_id) && Rc::ptr_eq(&a.order, &b.order)
                }
                (None, None) => true,
                _ => false,
            },
        )?;
        // The selection, draft, editing and hover events.
        *self.store.announced.borrow_mut() = Some(self.model());
        let me = self.weak();
        self.cx.on_state_change(move |_, _| {
            let Some(core) = me.upgrade() else { return Ok(()) };
            core.announce_session();
            Ok(())
        });
        Ok(())
    }

    /// `previous.order.filter((id) => !(id in next.byId))` leaves the session.
    fn forget_gone(
        &self,
        next: &crate::read::view::AnnotationView,
        previous: &crate::read::view::AnnotationView,
    ) -> Result<(), Error> {
        let gone: Vec<Id> =
            previous.order.iter().filter(|id| !next.by_id.contains_key(*id)).cloned().collect();
        if !gone.is_empty() {
            // A failed forget would leave a gesture on a record that is gone
            // (H3): it reaches the kernel's report, as the TypeScript's throw
            // in `ctx.watch` does.
            self.commit_json(json!({ "type": "forget", "ids": gone }))?;
        }
        Ok(())
    }

    fn announce_session(&self) {
        let next = self.model();
        let previous = self.store.announced.borrow_mut().replace(next.clone());
        let Some(previous) = previous else { return };
        if !same_ids(&previous.selected, &next.selected) {
            self.events.selection_changed.emit(&json!({
                "refs": refs_of_in(&next, &next.selected),
                "previousRefs": refs_of_in(&previous, &previous.selected),
            }));
        }
        let same_draft = match (&previous.draft, &next.draft) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same_draft {
            let anchor = epdf_annotation_core::view::creation_draft_anchor(&next);
            self.events.draft_changed.emit(&json!({ "draft": anchor.map_or(Value::Null, |a| epdf_annotation_core::object::Json::json(&a)) }));
        }
        if previous.editing != next.editing {
            let reference = next
                .editing
                .as_ref()
                .and_then(|id| next.by_id.get(id))
                .and_then(|r| r.annotation.get("ref").cloned());
            self.events.editing_changed.emit(&json!({ "ref": reference.unwrap_or(Value::Null) }));
        }
        if previous.hovered != next.hovered {
            let reference = next
                .hovered
                .as_ref()
                .and_then(|id| next.by_id.get(id))
                .and_then(|r| r.annotation.get("ref").cloned());
            self.events.hover_changed.emit(&json!({ "ref": reference.unwrap_or(Value::Null) }));
        }
    }

    // ── object numbers (`services/object-numbers.ts`) ───────────────────────

    /// `hold(count)`: make the session hold `count` object numbers, from the
    /// document's pool; false when it has too few.
    fn hold_numbers(&self, count: usize) -> bool {
        let held = self.state().session.object_numbers.len();
        if held >= count {
            return true;
        }
        self.watch_lost();
        let Ok(changes) = self.changes() else { return false };
        let mut taken: Vec<f64> = Vec::new();
        while held + taken.len() < count {
            match changes.take_object_number() {
                Ok(Some(number)) => taken.push(number as f64),
                _ => break,
            }
        }
        if !taken.is_empty() {
            self.update_state(|s| with_object_numbers(s, &taken));
        }
        held + taken.len() >= count
    }

    /// `take(count)`: numbers the session holds, for creates stated in code.
    fn take_numbers(&self, count: usize) -> Option<Vec<u32>> {
        if !self.hold_numbers(count) {
            return None;
        }
        let numbers = self.state().session.object_numbers.clone();
        let rest: Vec<f64> = numbers.iter().skip(count).copied().collect();
        self.update_state(|s| {
            let mut next = (**s).clone();
            next.session.object_numbers = Rc::new(rest);
            Rc::new(next)
        });
        Some(numbers.iter().take(count).map(|n| *n as u32).collect())
    }

    /// `numbers.named(ops, held)`: a number for every create that has none.
    /// Returns the ops and what the session holds after, `None` when that
    /// is `held` itself.
    fn named_ops(&self, ops: Vec<Value>, held: &[f64]) -> (Vec<Value>, Option<Vec<f64>>) {
        let mut left: Vec<f64> = held.to_vec();
        let mut changed = false;
        let out = ops
            .into_iter()
            .map(|mut op| {
                if op.get("type").and_then(Value::as_str) != Some("annotations.create")
                    || op
                        .get("objectNumber")
                        .is_some_and(|n| !epdf_kernel::is_undefined(n) && !n.is_null())
                {
                    return op;
                }
                let number = if left.is_empty() {
                    self.changes()
                        .ok()
                        .and_then(|c| c.take_object_number().ok().flatten())
                        .map(|n| n as f64)
                } else {
                    changed = true;
                    Some(left.remove(0))
                };
                if let Some(n) = number {
                    op["objectNumber"] = crate::js::num(n);
                }
                op
            })
            .collect();
        (out, if changed { Some(left) } else { None })
    }

    /// `watchLost()`: numbers the engine reclaimed leave the session too, as
    /// a create naming one would be refused. Subscribed once, the first time
    /// the session takes numbers.
    fn watch_lost(&self) {
        if self.store.watching_lost.get() {
            return;
        }
        let Some(doc) = self.doc() else { return };
        self.store.watching_lost.set(true);
        let me = self.weak();
        let off = doc.on_lost_object_numbers(Rc::new(move |lost| {
            if let Some(core) = me.upgrade() {
                let numbers: Vec<f64> = lost.numbers.iter().map(|n| f64::from(*n)).collect();
                core.update_state(|s| without_object_numbers(s, &numbers));
            }
        }));
        self.cx.listen(off);
    }

    // ── selection history (`services/selection-history.ts`) ────────────────

    fn remember(&self, op_id: &str, before: &[Id], after: &[Id]) {
        let mut kept = self.store.kept.borrow_mut();
        kept.insert(op_id.to_string(), (before.to_vec(), after.to_vec()));
        if kept.len() > KEPT {
            kept.shift_remove_index(0);
        }
    }

    fn on_staged(&self, change: &PendingChange) -> Result<(), Error> {
        let Some(undo_of) = change.undo_of() else { return Ok(()) };
        let undone = self.store.kept.borrow().get(&undo_of).cloned();
        let Some((before, after)) = undone else { return Ok(()) };
        self.remember(change.op_id(), &after, &before);
        let model = self.model();
        let ids: Vec<Id> = before.into_iter().filter(|id| model.by_id.contains_key(id)).collect();
        let message = if ids.is_empty() {
            json!({ "type": "deselect" })
        } else {
            json!({ "type": "select", "ids": ids })
        };
        self.commit_json(message).map(|_| ())
    }

    // ── the doors ───────────────────────────────────────────────────────────

    /// Stage this plugin's ops and its lender's as one change.
    fn stage(
        &self,
        ops: &[Value],
        before: &Rc<Value>,
        lent: &[Value],
    ) -> Result<PendingChange, Error> {
        let mut all = ops.to_vec();
        all.extend_from_slice(lent);
        let mut undo: Vec<Value> = Vec::new();
        if !lent.is_empty()
            && let Some(lender) = self.lending.lender()
        {
            undo.extend(lender.undo_of(lent));
        }
        undo.extend(undo_of(before, ops));
        let mut staged = StagedChange::ops(label_of(&all), all);
        staged.undo = undo;
        let change = self.changes()?.stage(staged)?;
        self.store.mine.borrow_mut().insert(change.op_id().to_string());
        Ok(change)
    }

    /// `hold(label, options)`: a change later commits go into.
    pub(crate) fn hold_change(
        &self,
        label: Value,
        merge: Option<String>,
    ) -> Result<StoreHold, Error> {
        let held = self.changes()?.hold(label, merge)?;
        Ok(StoreHold { held, base: Rc::default(), mine: self.store.mine.clone() })
    }

    /// `commit` with a message as JSON.
    pub(crate) fn commit_json(&self, message: Value) -> Result<Commit, Error> {
        self.commit(message, None, None)
    }

    /// `commit(message, { adjust, into })`.
    pub(crate) fn commit(
        &self,
        message: Value,
        adjust: Option<Adjust>,
        into: Option<&StoreHold>,
    ) -> Result<Commit, Error> {
        let parsed = Message::read(&message).map_err(|e| {
            plugin_error(
                code::INVALID_INPUT,
                CAPABILITY,
                format!("[annotation] {}", e.0),
                None,
                None,
            )
        })?;
        let needed = new_records_at_most(&parsed);
        if needed > 0 && !self.hold_numbers(needed) {
            return Ok(self.commit_when_numbered(message, adjust, into.cloned(), needed));
        }
        let before = self.model();
        let mut result =
            update(&before, &parsed).map_err(|e| to_plugin_error(CAPABILITY, &core_error(e)))?;
        if let Some(adjust) = &adjust {
            result = adjust(result);
        }
        let after = model_after(&before, &result);
        let shown = self.records.view();
        let lender = self.lending.lender();
        let is_lent = |effect: &Effect| {
            lender.is_some() && effect_id(effect).is_some_and(|id| self.lending.owns(id))
        };
        let lent_effects: Vec<Effect> =
            result.effects.iter().filter(|e| is_lent(e)).cloned().collect();
        let mut built: Vec<Value> = Vec::new();
        for effect in result.effects.iter().filter(|e| !is_lent(e)) {
            built.extend(self.build_effect(effect, &after, &before));
        }
        let built = self.with_follow_ups(built, &after);
        let lent = match (&lender, lent_effects.is_empty()) {
            (Some(lender), false) => lender.ops_of(&lent_effects, &after, &before),
            _ => Vec::new(),
        };
        let (named, held) = self.named_ops(built, &result.session.object_numbers);
        let ops = without_covered_deletes(&shown, named);
        if let Some(held) = held {
            result.session.object_numbers = Rc::new(held);
        }
        let mut written = Promise::resolved(NOTHING_WRITTEN());
        if !lent.is_empty() && into.is_none() {
            let change = self.stage(&ops, &shown, &lent)?;
            self.remember(change.op_id(), &before.selected, &result.session.selected);
            let mut all = ops.clone();
            all.extend(lent.iter().cloned());
            written = written_of(&change, ids_of(&all));
        } else if !ops.is_empty() {
            if let Some(into) = into {
                if into.base.borrow().is_none() {
                    *into.base.borrow_mut() = Some(shown.clone());
                }
                let base = into.base.borrow().clone().unwrap_or(shown.clone());
                into.held.set(ops.clone(), undo_of(&base, &ops));
                if let Some(change) = into.held.change() {
                    self.store.mine.borrow_mut().insert(change.op_id().to_string());
                    self.remember(change.op_id(), &before.selected, &result.session.selected);
                    written = written_of(&change, ids_of(&ops));
                }
            } else {
                let change = self.stage(&ops, &shown, &[])?;
                self.remember(change.op_id(), &before.selected, &result.session.selected);
                written = written_of(&change, ids_of(&ops));
            }
        }
        let vector: Vec<Id> = result
            .change
            .put
            .iter()
            .filter(|r| r.source == epdf_annotation_core::types::Source::Vector)
            .map(|r| r.id.clone())
            .collect();
        let session = result.session.clone();
        self.update_state(|s| with_session(s, &session, &vector));
        Ok(Commit { effects: result.effects, written })
    }

    /// A message that may create, run once the pool has the numbers it needs.
    fn commit_when_numbered(
        &self,
        message: Value,
        adjust: Option<Adjust>,
        into: Option<StoreHold>,
        needed: usize,
    ) -> Commit {
        let draft = self.model().draft.clone();
        let reserve = match self.changes() {
            Ok(c) => c.reserve_object_numbers(needed as u32),
            Err(e) => Promise::rejected(e),
        };
        let me = self.weak();
        let written = Promise::spawn(async move {
            match reserve.await {
                Ok(()) => {
                    let Some(core) = me.upgrade() else { return Ok(NOTHING_WRITTEN()) };
                    let now = core.model().draft.clone();
                    let same = match (&draft, &now) {
                        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                        (None, None) => true,
                        _ => false,
                    };
                    if !same {
                        return Ok(NOTHING_WRITTEN());
                    }
                    match core.commit(message, adjust, into.as_ref()) {
                        Ok(commit) => commit.written.await,
                        Err(error) => Err(error),
                    }
                }
                Err(error) => Ok(Written {
                    failed: vec![(Vec::new(), to_plugin_error(CAPABILITY, &error))],
                    result: None,
                }),
            }
        });
        Commit { effects: Vec::new(), written }
    }

    /// `apply(changes, { select })`: changes stated in code, shown at once as one change.
    pub(crate) fn apply(&self, changes: &[StoreChange], select: bool) -> Result<Applied, Error> {
        let shown = self.records.view();
        let creates = changes.iter().filter(|c| c.is_create()).count();
        let mut taken = if creates > 0 {
            self.take_numbers(creates).ok_or_else(no_numbers)?
        } else {
            Vec::new()
        };
        let mut ops: Vec<Value> = Vec::new();
        let mut lent: Vec<Value> = Vec::new();
        let mut op_index: Vec<Option<usize>> = Vec::new();
        let mut ids: Vec<Id> = Vec::new();
        let mut vector: Vec<Id> = Vec::new();
        let mut current = shown.clone();
        let model = self.model();
        let record_of = |id: &str| model.by_id.get(id).cloned();
        let run = (|| -> Result<(), Error> {
            for stated in changes {
                if let StoreChange::Update { reference, .. } | StoreChange::Delete { reference } =
                    stated
                {
                    self.lending.refuse_lent(reference)?;
                }
                let change = match stated {
                    StoreChange::Reorder { page, refs, position } => {
                        match self.lending.own_reorder(page, refs, position, &mut lent)? {
                            Some(own) => StoreChange::Reorder {
                                page: page.clone(),
                                refs: own,
                                position: position.clone(),
                            },
                            None => {
                                ids.push(refs.first().map(key_or_none).unwrap_or_default());
                                op_index.push(None);
                                continue;
                            }
                        }
                    }
                    other => other.clone(),
                };
                let checked = checked_op_of(&change, &current, &mut taken, &record_of)?;
                ids.push(checked.id.clone());
                let Some(op) = checked.op else {
                    op_index.push(None);
                    continue;
                };
                op_index.push(Some(ops.len()));
                if checked.live {
                    vector.push(checked.id);
                }
                current = predict_records(current.clone(), &op)?;
                ops.push(op);
            }
            Ok(())
        })();
        if let Err(error) = run {
            return Err(to_plugin_error(CAPABILITY, &error));
        }
        if ops.is_empty() && lent.is_empty() {
            let annotations = changes.iter().map(|_| None).collect();
            return Ok(Applied {
                ids,
                written: Promise::resolved(AppliedOutcome {
                    written: NOTHING_WRITTEN(),
                    annotations,
                }),
            });
        }
        // What follows each update joins the change, after the ops stated.
        let over = model_over(&self.model(), &shown, &current);
        let mut following: Vec<Value> = Vec::new();
        for op in &ops {
            if op.get("type").and_then(Value::as_str) == Some("annotations.update") {
                following.extend(self.follow_up(op, &over));
            }
        }
        let session_numbers = self.state().session.object_numbers.clone();
        let (following, held) = self.named_ops(following, &session_numbers);
        if let Some(held) = held {
            self.update_state(|s| {
                let mut next = (**s).clone();
                next.session.object_numbers = Rc::new(held.clone());
                Rc::new(next)
            });
        }
        let selected = self.model().selected.clone();
        let mut all = ops.clone();
        all.extend(following);
        let staged_ops = without_covered_deletes(&shown, all);
        let change = self.stage(&staged_ops, &shown, &lent)?;
        let after: Vec<Id> = if select { ids.clone() } else { (*selected).clone() };
        self.remember(change.op_id(), &selected, &after);
        if !vector.is_empty() {
            let session = self.state().session.clone();
            self.update_state(|s| with_session(s, &session, &vector));
        }
        if select {
            self.commit_json(json!({ "type": "select", "ids": ids }))?;
        }
        let mut named = ops.clone();
        named.extend(lent.iter().cloned());
        let written = written_of(&change, ids_of(&named));
        let written = Promise::spawn(async move {
            let outcome = written.await?;
            let annotations = op_index
                .iter()
                .map(|index| {
                    let item = (*index)
                        .and_then(|i| outcome.result.as_ref()?.get("items")?.get(i).cloned())?;
                    item.get("annotation").cloned()
                })
                .collect();
            Ok(AppliedOutcome { written: outcome, annotations })
        });
        Ok(Applied { ids, written })
    }

    /// `applyWhenNumbered(changes, options)`: `apply`, once the object
    /// numbers its creates take are there (at once, in this call, when they are).
    pub(crate) fn apply_when_numbered(
        &self,
        changes: Vec<StoreChange>,
        select: bool,
    ) -> Promise<Applied> {
        let creates = changes.iter().filter(|c| c.is_create()).count();
        let me = self.weak();
        Promise::spawn(async move {
            loop {
                let Some(core) = me.upgrade() else {
                    return Err(Error::new("[annotation] closed"));
                };
                if core.hold_numbers(creates) {
                    return core.apply(&changes, select);
                }
                let reserve = core.changes()?.reserve_object_numbers(creates as u32);
                drop(core);
                reserve.await?;
            }
        })
    }

    /// `whenWritten()`: resolves once every change staged so far has its answer.
    pub(crate) fn when_written(&self) -> Promise<()> {
        match self.changes() {
            Ok(c) => c.when_settled(),
            Err(_) => Promise::resolved(()),
        }
    }

    /// One effect as the engine's ops (the op builders, `write/ops.ts` and
    /// the areas that own `text`, `syncLink` and `captured`).
    fn build_effect(&self, effect: &Effect, model: &Model, before: &Model) -> Vec<Value> {
        let ref_in =
            |m: &Model, id: &str| m.by_id.get(id).and_then(|r| r.annotation.get("ref").cloned());
        match effect {
            Effect::Create(e) => {
                let Some(reference) = ref_in(model, &e.id) else { return Vec::new() };
                if reference.get("kind").and_then(Value::as_str) != Some("objectNumber") {
                    return Vec::new();
                }
                vec![json!({
                    "type": "annotations.create",
                    "page": reference.get("page").cloned().unwrap_or(Value::Null),
                    "data": e.draft,
                    "objectNumber": reference.get("objectNumber").cloned().unwrap_or(Value::Null),
                })]
            }
            Effect::Patch(e) => match ref_in(model, &e.id) {
                Some(reference) => vec![
                    json!({ "type": "annotations.update", "ref": reference, "patch": e.patch }),
                ],
                None => Vec::new(),
            },
            Effect::Delete(e) => match ref_in(before, &e.id) {
                Some(reference) => vec![json!({ "type": "annotations.delete", "ref": reference })],
                None => Vec::new(),
            },
            Effect::Text(e) => self.text_ops(&e.id, model),
            Effect::SyncLink(e) => self
                .child_changes(&e.id, &e.target, model)
                .iter()
                .map(crate::services::staging::op_of_stated)
                .collect(),
            Effect::Captured(e) => {
                self.capture(e);
                Vec::new()
            }
        }
    }

    /// `withFollowUps(ops, followUp, model)`: what follows each update right after it.
    fn with_follow_ups(&self, ops: Vec<Value>, model: &Model) -> Vec<Value> {
        let mut out = Vec::with_capacity(ops.len());
        for op in ops {
            let is_update = op.get("type").and_then(Value::as_str) == Some("annotations.update");
            let follow = if is_update { self.follow_up(&op, model) } else { Vec::new() };
            out.push(op);
            out.extend(follow);
        }
        out
    }
}

/// The id an effect names, for one that names a record.
fn effect_id(effect: &Effect) -> Option<&str> {
    match effect {
        Effect::Create(e) => Some(&e.id),
        Effect::Patch(e) => Some(&e.id),
        Effect::Text(e) | Effect::Delete(e) => Some(&e.id),
        Effect::SyncLink(e) => Some(&e.id),
        Effect::Captured(_) => None,
    }
}

/// The refs a refused change named, as `writeFailed` reports them.
fn refs_named_by(op: &Value) -> Vec<Value> {
    let array = |key: &str| op.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
    match op.get("type").and_then(Value::as_str).unwrap_or("") {
        "annotations.create" => {
            match op.get("objectNumber").filter(|n| !epdf_kernel::is_undefined(n)) {
                Some(n) => vec![
                    json!({ "kind": "objectNumber", "page": op.get("page").cloned().unwrap_or(Value::Null), "objectNumber": n }),
                ],
                None => Vec::new(),
            }
        }
        "annotations.update" | "annotations.delete" => {
            vec![op.get("ref").cloned().unwrap_or(Value::Null)]
        }
        "forms.updateWidget" | "forms.deleteWidget" => {
            vec![op.get("widget").cloned().unwrap_or(Value::Null)]
        }
        "annotations.reorder" => array("refs"),
        "forms.reorderWidgets" => array("widgets"),
        _ => Vec::new(),
    }
}
