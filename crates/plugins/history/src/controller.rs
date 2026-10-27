//! `controller.ts`: the history controller. It records every change this
//! session stages on the document's change queue, as it is staged, so an
//! undo right after an action undoes that action even before the engine
//! answered it. Undo and redo are changes on the same queue: the undo of a
//! change, by its `opId`. The views show them at once, from what the change
//! said undoing it looks like, and the engine applies the reverse it
//! recorded.
//!
//! The history keeps no copy of the document. What it holds is names,
//! labels and what each step looks like, and it changes on four things: a
//! change staged, a step staged, an answer, and a change that ends undo (a
//! final change, a new version). Getters and events follow the engine's
//! answer; nothing here restores a snapshot.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use epdf_kernel::changes::{ChangeQueue, PendingChange, SettledChange, StagedChange};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::plugin::{ConnectFn, Controller};
use epdf_kernel::settings::InstanceSettings;
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{Error, Promise, rt};
use serde_json::Value;

use crate::contract::*;
use crate::model::{
    self, ChangeLabel, Direction, History, HistoryEntry, HistoryPart, RecordedChange,
};

/// `ENDS_UNDO`: the document events after which nothing before them can be undone.
const ENDS_UNDO: [&str; 5] = [
    "redaction.applied",
    "pages.flattened",
    "signatures.completed",
    "forms.repaired",
    "document.versioned",
];

/// `TypeError: Cannot read properties of <what> (reading '<member>')`.
fn type_error(value: Option<&Value>, member: &str) -> Error {
    let what = if value.is_some_and(Value::is_null) { "null" } else { "undefined" };
    Error::named("TypeError", format!("Cannot read properties of {what} (reading '{member}')"))
}

/// JavaScript truthiness of a JSON member (`undefined` when absent).
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(v) if epdf_kernel::is_undefined(v) => false,
        Some(_) => true,
    }
}

/// `result.meta.undoable`, read as the TypeScript reads it.
fn undoable(result: &Value) -> Result<bool, Error> {
    match result.get("meta").filter(|m| !m.is_null() && !epdf_kernel::is_undefined(m)) {
        Some(meta) => Ok(truthy(meta.get("undoable"))),
        None => Err(type_error(result.get("meta"), "undoable")),
    }
}

/// `isUnavailable(error)`: the engine refused an undo because it can't be
/// undone any more. `EngineError.is(cause, UndoUnavailable)` reads the
/// cause's name and code; the kernel keeps the cause in canonical form.
fn is_unavailable(error: &Error) -> bool {
    error.member("cause").and_then(|cause| cause.get("$error")).is_some_and(|cause| {
        cause.get("name").and_then(Value::as_str) == Some("EngineError")
            && cause.get("code").and_then(Value::as_str) == Some("UndoUnavailable")
    })
}

/// `skippedIn(result)`: how many of an undo's parts were left alone, in
/// whole or in part.
fn skipped_in(result: &Value) -> Result<u64, Error> {
    let Some(items) = result.get("items").and_then(Value::as_array) else {
        return Err(Error::named("TypeError", "result.items.filter is not a function"));
    };
    Ok(items
        .iter()
        .filter(|item| {
            if item.get("type").and_then(Value::as_str) == Some("skipped") {
                return true;
            }
            // `'skipped' in item && (item.skipped?.length ?? 0) > 0`.
            match item.get("skipped") {
                Some(Value::Array(a)) => !a.is_empty(),
                Some(Value::String(s)) => !s.is_empty(),
                _ => false,
            }
        })
        .count() as u64)
}

/// JavaScript `ToNumber` of a JSON setting, for `Math.max(1, limit)`.
fn to_number(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() { 0.0 } else { s.parse().unwrap_or(f64::NAN) }
        }
        Some(Value::Object(o)) if o.contains_key("$number") => {
            match o["$number"].as_str().unwrap_or("") {
                "Infinity" => f64::INFINITY,
                "-Infinity" => f64::NEG_INFINITY,
                _ => f64::NAN,
            }
        }
        _ => f64::NAN,
    }
}

fn settings_of(value: &Value) -> HistorySettings {
    HistorySettings { limit: to_number(value.get("limit")) }
}

/// What one part's answer did to the step. `Moot`: it named a change the
/// engine never applied, or a part the history forgot, so its refusal says
/// nothing new.
#[derive(Clone)]
enum Answer {
    Skipped(u64),
    Failed(Error),
    Moot,
}

struct Inner {
    cx: Controller<History, Value>,
    changes: ChangeQueue,
    settings: InstanceSettings,
    undone: EventHookSource<HistoryUndoneEvent>,
    undo_failed: EventHookSource<HistoryUndoFailedEvent>,
    /// The changes the engine will never apply, while a step is on its way:
    /// a refused change, and a step that undid one. A step that names one
    /// is refused too, and changes nothing more.
    dead: RefCell<HashSet<String>>,
    /// The parts forgotten while a step is on its way: a refusal of their
    /// step puts nothing back.
    gone: RefCell<HashSet<String>>,
    in_flight: Cell<u32>,
}

impl Inner {
    fn state(&self) -> Rc<History> {
        self.cx.state()
    }

    fn update(&self, transition: impl FnOnce(Rc<History>) -> Rc<History>) {
        self.cx.update_state(transition);
    }

    fn forget<'a>(&self, ids: impl IntoIterator<Item = &'a str>) {
        if self.in_flight.get() == 0 {
            return;
        }
        let mut gone = self.gone.borrow_mut();
        for id in ids {
            gone.insert(id.to_string());
        }
    }

    fn clear(&self) {
        let history = self.state();
        self.forget(
            parts_of(history.undo.iter().chain(history.redo.iter())).iter().map(String::as_str),
        );
        self.update(|h| model::cleared(&h));
    }

    /// `openHold()`: a change still being typed, what undo would undo before
    /// anything in the history.
    fn open_hold(&self) -> Option<PendingChange> {
        self.changes.pending().into_iter().find(|change| change.held() && change.history())
    }

    /// `answerOf(direction, entry, part, change)`: what one step's change for
    /// one part answered, a `.then` reaction on its result.
    fn answer_of(
        self: &Rc<Self>,
        direction: Direction,
        entry: HistoryEntry,
        part: HistoryPart,
        change: PendingChange,
    ) -> Promise<Answer> {
        let this = self.clone();
        let result = change.result();
        Promise::spawn(async move {
            match result.reaction().await {
                Ok(result) => {
                    if !undoable(&result)? {
                        // It did nothing (every part was left alone): there
                        // is nothing to step back over.
                        this.forget([part.id.as_str()]);
                        this.update(|h| model::forgotten(&h, &part.id));
                    }
                    Ok(Answer::Skipped(skipped_in(&result)?))
                }
                Err(error) => {
                    this.dead.borrow_mut().insert(change.op_id().to_string());
                    if this.dead.borrow().contains(&part.op_id)
                        || this.gone.borrow().contains(&part.id)
                    {
                        return Ok(Answer::Moot);
                    }
                    if is_unavailable(&error) {
                        let history = this.state();
                        let older: Vec<&HistoryEntry> = history
                            .undo
                            .iter()
                            .chain(history.redo.iter())
                            .filter(|other| other.seq <= entry.seq)
                            .collect();
                        this.forget(parts_of(older).iter().map(String::as_str));
                        this.update(|h| model::unavailable(&h, entry.seq));
                    } else {
                        // The part didn't move: it goes back, and the step can
                        // be tried again.
                        this.update(|h| model::step_refused(&h, direction, &entry, &part));
                    }
                    Ok(Answer::Failed(error))
                }
            }
        })
    }

    /// `step(direction)`: the undo of each part of the next entry, as one
    /// change each (undo goes newest first, redo oldest first), staged now,
    /// so the views show it at once.
    fn step(self: &Rc<Self>, direction: Direction) -> Result<(), Error> {
        // What is still being typed is part of the history from now on: send it first.
        self.changes.send_holds();
        let Some(entry) = model::next_of(&self.state(), direction).cloned() else {
            return Ok(());
        };
        let mut parts = entry.parts.clone();
        if direction == Direction::Undo {
            parts.reverse();
        }
        let mut staged: Vec<(HistoryPart, PendingChange)> = Vec::new();
        for part in parts {
            let shows = match direction {
                Direction::Undo => part.backward.clone(),
                Direction::Redo => part.forward.clone(),
            };
            match self.changes.stage(StagedChange::undo(
                entry.label.clone(),
                part.op_id.clone(),
                shows,
            )) {
                Ok(change) => staged.push((part, change)),
                // The document closed: the parts staged so far move, the rest stay.
                Err(error) if staged.is_empty() => return Err(error),
                Err(_) => break,
            }
        }
        // The first stage either succeeded or returned its error above.
        debug_assert!(!staged.is_empty(), "a step stages at least one part");
        let op_ids: HashMap<String, String> = staged
            .iter()
            .map(|(part, change)| (part.id.clone(), change.op_id().to_string()))
            .collect();
        self.update(|h| model::stepped(&h, direction, entry.seq, &op_ids));
        self.in_flight.set(self.in_flight.get() + 1);

        let answers: Vec<Promise<Answer>> = staged
            .into_iter()
            .map(|(part, change)| self.answer_of(direction, entry.clone(), part, change))
            .collect();
        // `Promise.all(answers).then(...)`: each element's reaction follows its
        // answer by a turn, and the `.then` by one more.
        let count = answers.len();
        let collected: Rc<RefCell<Vec<Option<Answer>>>> = Rc::new(RefCell::new(vec![None; count]));
        let left = Rc::new(Cell::new(count));
        let failed = Rc::new(Cell::new(false));
        for (index, answer) in answers.into_iter().enumerate() {
            let (this, collected, left, failed, entry) =
                (self.clone(), collected.clone(), left.clone(), failed.clone(), entry.clone());
            answer.on_settle(move |result| {
                match result {
                    Ok(answer) => collected.borrow_mut()[index] = Some(answer.clone()),
                    // `Promise.all` rejects: its `.then` never runs.
                    Err(_) => failed.set(true),
                }
                left.set(left.get() - 1);
                if left.get() > 0 || failed.get() {
                    return;
                }
                rt::spawn(async move {
                    rt::tick().await;
                    rt::tick().await;
                    let answers: Vec<Answer> =
                        collected.borrow_mut().drain(..).map(Option::unwrap).collect();
                    this.settled(direction, &entry, answers);
                });
            });
        }
        Ok(())
    }

    /// The step's answers, all in.
    fn settled(&self, direction: Direction, entry: &HistoryEntry, answers: Vec<Answer>) {
        self.in_flight.set(self.in_flight.get() - 1);
        if self.in_flight.get() == 0 {
            self.dead.borrow_mut().clear();
            self.gone.borrow_mut().clear();
        }
        let redo = direction == Direction::Redo;
        if answers.iter().all(|answer| matches!(answer, Answer::Moot)) {
            return;
        }
        if let Some(error) = answers.iter().find_map(|answer| match answer {
            Answer::Failed(error) => Some(error),
            _ => None,
        }) {
            self.undo_failed.emit(&HistoryUndoFailedEvent {
                label: entry.label.clone(),
                redo,
                reason: if is_unavailable(error) {
                    UndoFailure::Unavailable
                } else {
                    UndoFailure::Refused
                },
                error: error.clone(),
            });
            return;
        }
        let skipped = answers
            .iter()
            .map(|answer| match answer {
                Answer::Skipped(n) => *n,
                _ => 0,
            })
            .sum();
        self.undone.emit(&HistoryUndoneEvent { label: entry.label.clone(), redo, skipped });
    }
}

/// `partsOf(entries)`: every part's id.
fn parts_of<'a>(entries: impl IntoIterator<Item = &'a HistoryEntry>) -> Vec<String> {
    entries.into_iter().flat_map(|entry| entry.parts.iter().map(|part| part.id.clone())).collect()
}

/// `HistoryCapability`: one undo and redo history per open document. Cheap to clone.
#[derive(Clone)]
pub struct HistoryCapability(Rc<Inner>);

impl HistoryCapability {
    // ── settings (`SettingsApi<HistorySettings>`) ─────────────────────────

    /// The settings in effect.
    pub fn get_settings(&self) -> HistorySettings {
        settings_of(&self.0.settings.get())
    }

    /// The settings as the kernel's JSON store holds them, which is what the
    /// TypeScript `getSettings()` returns.
    pub fn settings_json(&self) -> Rc<Value> {
        self.0.settings.get()
    }

    /// Merge `changes` into the settings, for every document.
    pub fn update_settings(&self, changes: &HistoryConfig) {
        self.update_settings_json(&serde_json::to_value(changes).unwrap_or_default());
    }

    /// [`update_settings`](Self::update_settings) with the change as JSON.
    pub fn update_settings_json(&self, changes: &Value) {
        self.0.settings.store.update_settings(changes);
    }

    pub fn reset_settings(&self) {
        self.0.settings.store.reset_settings();
    }

    /// The JSON store's changes, for this instance's lifetime.
    pub fn on_settings_changed(
        &self,
        listener: impl Fn(&HistorySettingsChanged) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.settings.events.on(move |event| {
            listener(&HistorySettingsChanged {
                settings: settings_of(&event.settings),
                changed: event.changed.clone(),
            })
        })
    }

    /// The raw store event, for a caller that prints the settings JSON.
    pub fn on_settings_changed_json(
        &self,
        listener: impl Fn(&Value, &[String]) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.settings.events.on(move |event| listener(&event.settings, &event.changed))
    }

    // ── history ───────────────────────────────────────────────────────────

    /// Undo the last step, at once: the document shows it undone straight
    /// away, and the engine is asked after everything staged before it. A
    /// step whose change is still on its way is undone the same way. Does
    /// nothing when there is nothing to undo. `on_undone` or
    /// `on_undo_failed` follows once the engine answered. Fails only when
    /// the document closed before any part could be staged.
    pub fn undo(&self) -> Result<(), Error> {
        self.0.step(Direction::Undo)
    }

    /// Redo the last step undone, the same way.
    pub fn redo(&self) -> Result<(), Error> {
        self.0.step(Direction::Redo)
    }

    pub fn can_undo(&self) -> bool {
        self.0.open_hold().is_some() || !self.0.state().undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        self.0.open_hold().is_none() && !self.0.state().redo.is_empty()
    }

    /// What undo would undo, for a tooltip ("Undo move of 3 annotations").
    pub fn get_undo_label(&self) -> Option<ChangeLabel> {
        if let Some(hold) = self.0.open_hold() {
            return Some(hold.label().clone());
        }
        model::next_of(&self.0.state(), Direction::Undo).map(|entry| entry.label.clone())
    }

    /// What redo would redo.
    pub fn get_redo_label(&self) -> Option<ChangeLabel> {
        if self.0.open_hold().is_some() {
            return None;
        }
        model::next_of(&self.0.state(), Direction::Redo).map(|entry| entry.label.clone())
    }

    /// Forget every step: nothing can be undone or redone until the next change.
    pub fn clear(&self) {
        self.0.clear();
    }

    pub fn on_undone(
        &self,
        listener: impl Fn(&HistoryUndoneEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.undone.on(listener)
    }

    pub fn on_undo_failed(
        &self,
        listener: impl Fn(&HistoryUndoFailedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.undo_failed.on(listener)
    }
}

/// `createHistoryController(ctx)`.
pub(crate) fn create_history_controller(
    cx: Controller<History, Value>,
) -> Result<(HistoryCapability, Option<ConnectFn>), Error> {
    let settings = cx.settings()?;
    let changes = cx.changes()?;
    let inner = Rc::new(Inner {
        undone: cx.event_source(),
        undo_failed: cx.event_source(),
        settings,
        changes,
        cx,
        dead: RefCell::default(),
        gone: RefCell::default(),
        in_flight: Cell::new(0),
    });
    let weak = Rc::downgrade(&inner);
    let connect: ConnectFn = Rc::new(move || {
        let Some(inner) = weak.upgrade() else { return Ok(()) };
        connect(&inner, weak.clone())
    });
    Ok((HistoryCapability(inner), Some(connect)))
}

fn connect(inner: &Rc<Inner>, weak: Weak<Inner>) -> Result<(), Error> {
    // A change this session staged is a step of the history, unless it moves
    // along the history (an undo or a redo) or isn't one (`history: false`).
    let w = weak.clone();
    inner.cx.listen(inner.changes.on_staged().on(move |change| {
        let Some(inner) = w.upgrade() else { return Ok(()) };
        if change.undo_of().is_some() || !change.history() {
            return Ok(());
        }
        let recorded = RecordedChange {
            op_id: change.op_id().to_string(),
            label: change.label().clone(),
            merge: change.merge().map(str::to_string),
            shows: change.shows(),
            undo: change.undo(),
        };
        let limit = to_number(inner.settings.get().get("limit"));
        inner.update(|h| model::recorded(&h, &recorded, limit));
        Ok(())
    }));
    let w = weak.clone();
    inner.cx.listen(inner.changes.on_settled().on(move |settled| {
        let Some(inner) = w.upgrade() else { return Ok(()) };
        let change = match settled {
            SettledChange::Applied { change, .. } | SettledChange::Refused { change, .. } => change,
        };
        if change.undo_of().is_some() || !change.history() {
            return Ok(());
        }
        // A change the engine refused never happened; one that did nothing can't be undone.
        let refused = matches!(settled, SettledChange::Refused { .. });
        let forget = match settled {
            SettledChange::Refused { .. } => true,
            SettledChange::Applied { result, .. } => !undoable(result)?,
        };
        if forget {
            if refused && inner.in_flight.get() > 0 {
                inner.dead.borrow_mut().insert(change.op_id().to_string());
            }
            inner.forget([change.op_id()]);
            inner.update(|h| model::forgotten(&h, change.op_id()));
        }
        Ok(())
    }));
    let w = weak;
    let doc = inner.cx.doc()?;
    inner.cx.listen(doc.subscribe(Rc::new(move |event: &Value| {
        let Some(inner) = w.upgrade() else { return };
        if event.get("type").and_then(Value::as_str).is_some_and(|t| ENDS_UNDO.contains(&t)) {
            inner.clear();
        }
    })));
    Ok(())
}
