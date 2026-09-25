//! `capability.ts`: the twins, the tool verbs, marking from the selection,
//! the pending projection, the collateral estimate, labels, and the apply
//! flow over the engine's optional `redaction` service.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use epdf_annotation_core::AnnotationRef;
use epdf_annotation_core::js::Nullable;
use epdf_geometry::js as num;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::{Ctx, Error, is_undefined};
use epdf_plugin_annotation::promise::{all, await_engine};
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};
use epdf_plugin_interaction::INTERACTION;
use epdf_plugin_selection::SELECTION;
use serde_json::{Map, Value, json};

use crate::types::{
    AppliedCallback, PendingKind, RedactionAction, RedactionApplyResult, RedactionHost,
    RedactionLabelPatch, RedactionPendingItem, RedactionState,
};

/// Applying destroys content: its own granted power, narrower than
/// annotate. The engine's apply asserts all three (the redact grant, the
/// page-content rewrite, the annotation consumption); `can_apply` mirrors
/// the full set.
pub const APPLY_CAPABILITIES: [&str; 3] = ["doc.redact", "doc.pages.modify", "doc.annotate.modify"];

// ── pure helpers ──────────────────────────────────────────────────────────────

/// `String(value)` for the members a ref carries. Canonical JSON never
/// carries an `undefined` member (it omits it), so absence is the one
/// spelling of `undefined` here.
fn js_string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => num::number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => s.clone(),
        // `Array.prototype.join`: `null` elements print empty.
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| if item.is_null() { String::new() } else { js_string(Some(item)) })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(o)) => match o.get("$number").and_then(Value::as_str) {
            Some(spelled) => spelled.to_string(),
            None => "[object Object]".into(),
        },
    }
}

/// The annotation plugin's model-id convention for a durable ref.
pub fn id_of(r: &Value) -> String {
    match r.get("kind").and_then(Value::as_str) {
        Some("objectNumber") => format!("obj:{}", js_string(r.get("annotObjectNumber"))),
        Some("nm") => format!("nm:{}", js_string(r.get("nm"))),
        _ => format!("idx:{}:{}", js_string(r.get("pageObjectNumber")), js_string(r.get("index"))),
    }
}

/// A `PdfRect` read as the TypeScript reads its members: a missing or
/// non-numeric member is `NaN` to every comparison.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfRect {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

fn f64_of(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

impl PdfRect {
    pub fn of(v: &Value) -> PdfRect {
        PdfRect {
            left: f64_of(v, "left"),
            bottom: f64_of(v, "bottom"),
            right: f64_of(v, "right"),
            top: f64_of(v, "top"),
        }
    }
}

fn quad_points(dto: &Value) -> &[Value] {
    dto.get("quadPoints").and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// The PDF-space regions a redact mark targets: its quads' boxes, else
/// `/Rect`.
pub fn regions_of(dto: &Value) -> Vec<PdfRect> {
    let quads = quad_points(dto);
    if quads.is_empty() {
        return vec![PdfRect::of(dto.get("rect").unwrap_or(&Value::Null))];
    }
    quads
        .iter()
        .map(|q| {
            let corner = |p: &str, axis: &str| q.get(p).map_or(f64::NAN, |p| f64_of(p, axis));
            let xs = [corner("p1", "x"), corner("p2", "x"), corner("p3", "x"), corner("p4", "x")];
            let ys = [corner("p1", "y"), corner("p2", "y"), corner("p3", "y"), corner("p4", "y")];
            PdfRect {
                left: num::min_of(&xs),
                right: num::max_of(&xs),
                bottom: num::min_of(&ys),
                top: num::max_of(&ys),
            }
        })
        .collect()
}

/// Positive-area intersection: the engine's collateral rule.
pub fn intersects(a: &PdfRect, b: &PdfRect) -> bool {
    num::min(a.right, b.right) > num::max(a.left, b.left)
        && num::min(a.top, b.top) > num::max(a.bottom, b.bottom)
}

fn is_redact(dto: &Value) -> bool {
    dto.get("subtype").and_then(Value::as_str) == Some("redact")
}

/// `pages.map((p) => p.pageObjectNumber)` over the document's page list.
fn pons_of(pages: &[Value]) -> Vec<u32> {
    pages
        .iter()
        .filter_map(|p| p.get("pageObjectNumber").and_then(Value::as_u64).map(|n| n as u32))
        .collect()
}

/// `pages.find((p) => p.pageObjectNumber === pon)?.index ?? -1`.
fn page_index_of(pages: &[Value], pon: u32) -> f64 {
    pages
        .iter()
        .find(|p| p.get("pageObjectNumber").and_then(Value::as_u64) == Some(u64::from(pon)))
        .and_then(|p| p.get("index").and_then(Value::as_f64))
        .unwrap_or(-1.0)
}

/// `collectPending` over a page list and the annotation plane's `list`.
pub fn collect_pending(
    pages: &[Value],
    list: &dyn Fn(u32) -> Vec<Value>,
) -> Vec<RedactionPendingItem> {
    let mut items = Vec::new();
    for pon in pons_of(pages) {
        for dto in list(pon).into_iter().filter(is_redact) {
            let r = dto.get("ref").cloned().unwrap_or(Value::Null);
            items.push(RedactionPendingItem {
                id: id_of(&r),
                ref_: r,
                page_object_number: pon,
                page_index: page_index_of(pages, pon),
                kind: if quad_points(&dto).is_empty() {
                    PendingKind::Area
                } else {
                    PendingKind::Text
                },
                overlay_text: dto.get("overlayText").filter(|v| !is_undefined(v)).cloned(),
            });
        }
    }
    items
}

/// `estimateCollateral` over a page list and the annotation plane's `list`:
/// how many non-redact annotations the given marks (default: all) would
/// take with them.
pub fn estimate_collateral(
    pages: &[Value],
    list: &dyn Fn(u32) -> Vec<Value>,
    ids: Option<&[String]>,
) -> u32 {
    let wanted: Option<HashSet<&str>> = ids.map(|ids| ids.iter().map(String::as_str).collect());
    let mut count = 0;
    for pon in pons_of(pages) {
        let all = list(pon);
        let marks: Vec<&Value> = all
            .iter()
            .filter(|a| {
                is_redact(a)
                    && wanted.as_ref().is_none_or(|w| {
                        w.contains(id_of(a.get("ref").unwrap_or(&Value::Null)).as_str())
                    })
            })
            .collect();
        if marks.is_empty() {
            continue;
        }
        let regions: Vec<PdfRect> = marks.iter().flat_map(|m| regions_of(m)).collect();
        for other in &all {
            if is_redact(other) {
                continue;
            }
            let rect = PdfRect::of(other.get("rect").unwrap_or(&Value::Null));
            if regions.iter().any(|r| intersects(r, &rect)) {
                count += 1;
            }
        }
    }
    count
}

// ── the listener set ──────────────────────────────────────────────────────────

/// `new Set<(result) => void>()`, iterated as `for...of` does: a listener
/// added during a pass runs in that pass, one removed before its turn does
/// not.
#[derive(Clone, Default)]
struct AppliedCallbacks(Rc<CallbacksInner>);

#[derive(Default)]
struct CallbacksInner {
    entries: RefCell<Vec<Option<AppliedCallback>>>,
    walking: Cell<usize>,
}

fn same(a: &AppliedCallback, b: &AppliedCallback) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

impl AppliedCallbacks {
    fn add(&self, callback: AppliedCallback) {
        let mut entries = self.0.entries.borrow_mut();
        if !entries.iter().flatten().any(|e| same(e, &callback)) {
            entries.push(Some(callback));
        }
    }

    fn delete(&self, callback: &AppliedCallback) -> bool {
        let mut entries = self.0.entries.borrow_mut();
        let Some(slot) = entries.iter_mut().find(|e| e.as_ref().is_some_and(|e| same(e, callback)))
        else {
            return false;
        };
        *slot = None;
        if self.0.walking.get() == 0 {
            entries.retain(Option::is_some);
        }
        true
    }

    fn for_each(&self, mut visit: impl FnMut(&AppliedCallback)) {
        self.0.walking.set(self.0.walking.get() + 1);
        let mut index = 0;
        loop {
            let next = {
                let entries = self.0.entries.borrow();
                if index >= entries.len() {
                    break;
                }
                entries[index].clone()
            };
            index += 1;
            if let Some(callback) = next {
                visit(&callback);
            }
        }
        self.0.walking.set(self.0.walking.get() - 1);
        if self.0.walking.get() == 0 {
            self.0.entries.borrow_mut().retain(Option::is_some);
        }
    }
}

// ── the capability ────────────────────────────────────────────────────────────

struct Inner {
    ctx: Ctx<RedactionState, RedactionAction>,
    anno: AnnotationCapability,
    applied: AppliedCallbacks,
    host: RedactionHost,
}

/// `RedactionCapability`. Cheap to clone.
#[derive(Clone)]
pub struct RedactionCapability(Rc<Inner>);

fn fire_applied(applied: &AppliedCallbacks, host: &RedactionHost, result: &RedactionApplyResult) {
    applied.for_each(|cb| {
        if let Err(error) = cb(result)
            && let Some(console_error) = &host.console_error
        {
            console_error("[redaction] onApplied callback failed", &error);
        }
    });
}

/// `createRedactionCapability(ctx)`: resolves the annotation plane and
/// subscribes to the document's event stream for remote applies.
pub fn create_redaction_capability(
    ctx: Ctx<RedactionState, RedactionAction>,
    host: RedactionHost,
) -> Result<RedactionCapability, Error> {
    let anno = ctx.get(&ANNOTATION)?;
    let applied = AppliedCallbacks::default();

    // A REMOTE collaborator's apply arrives only as a document event (own
    // applies flow through `run_apply`). Annotation state and raster
    // invalidation are handled by their own planes; this only surfaces it.
    if let Some(doc) = ctx.doc() {
        let (listen_ctx, listen_applied, listen_host) =
            (ctx.clone(), applied.clone(), host.clone());
        let unsubscribe = doc.subscribe(Rc::new(move |event: &Value| {
            if event.get("type").and_then(Value::as_str) != Some("redaction.applied") {
                return;
            }
            let origin = event.get("origin").and_then(|o| o.get("kind")).and_then(Value::as_str);
            if origin != Some("remote") {
                return;
            }
            listen_ctx.dispatch(RedactionAction::ApplyFinished { result: Some(event.clone()) });
            fire_applied(&listen_applied, &listen_host, event);
        }));
        ctx.cleanup_fn(move || unsubscribe.call());
    }

    Ok(RedactionCapability(Rc::new(Inner { ctx, anno, applied, host })))
}

impl RedactionCapability {
    fn require_doc(&self) -> Result<Rc<dyn DocumentHandle>, Error> {
        self.0.ctx.doc().ok_or_else(|| Error::new("[redaction] no document bound"))
    }

    fn require_service(&self) -> Result<Rc<dyn DocumentHandle>, Error> {
        let doc = self.require_doc()?;
        if !doc.has("redaction") {
            return Err(Error::new("[redaction] engine has no redaction service"));
        }
        Ok(doc)
    }

    fn pages(&self) -> Vec<Value> {
        self.0.ctx.document().and_then(|d| d.pages.as_array().cloned()).unwrap_or_default()
    }

    fn pons(&self) -> Vec<u32> {
        pons_of(&self.pages())
    }

    fn list(&self) -> impl Fn(u32) -> Vec<Value> + '_ {
        |pon| self.0.anno.list(pon)
    }

    fn collect_pending(&self) -> Vec<RedactionPendingItem> {
        collect_pending(&self.pages(), &self.list())
    }

    /// `runApply(scope)`: the async body, whose promise `apply` and
    /// `applyAll` return from their own `async` bodies.
    fn run_apply(&self, scope: Value) -> Promise<RedactionApplyResult> {
        let this = self.clone();
        Promise::spawn(async move {
            let doc = this.require_service()?;
            this.0.ctx.dispatch(RedactionAction::ApplyStarted);
            match await_engine(doc.call("redaction.apply", None, vec![scope])).await {
                Ok(result) => {
                    this.0
                        .ctx
                        .dispatch(RedactionAction::ApplyFinished { result: Some(result.clone()) });
                    fire_applied(&this.0.applied, &this.0.host, &result);
                    Ok(result)
                }
                Err(error) => {
                    this.0.ctx.dispatch(RedactionAction::ApplyFinished { result: None });
                    Err(error)
                }
            }
        })
    }

    /// Marking authority IS annotation create authority: marks are
    /// annotations.
    pub fn can_mark(&self) -> bool {
        self.0.anno.can_create()
    }

    /// Engine support present and every capability the engine's apply
    /// asserts.
    pub fn can_apply(&self) -> bool {
        let Some(doc) = self.0.ctx.doc() else { return false };
        if !doc.has("redaction") {
            return false;
        }
        let security = doc.security();
        APPLY_CAPABILITIES.iter().all(|cap| security.allows(cap))
    }

    pub fn is_applying(&self) -> bool {
        self.0.ctx.state().applying
    }

    pub fn last_result(&self) -> Option<RedactionApplyResult> {
        self.0.ctx.state().last_result.clone()
    }

    /// Activate the composed marking tool. Throws the hub's unknown-tool
    /// error when the annotation plugin did not register it (no selection
    /// plugin).
    pub fn enable_redact(&self) -> Result<(), Error> {
        match self.0.ctx.try_get(&INTERACTION)? {
            Some(interaction) => interaction.activate_tool("redact"),
            None => Ok(()),
        }
    }

    pub fn toggle_redact(&self) -> Result<(), Error> {
        let Some(interaction) = self.0.ctx.try_get(&INTERACTION)? else { return Ok(()) };
        let next = if interaction.active_tool_id() == "redact" { "pointer" } else { "redact" };
        interaction.activate_tool(next)
    }

    pub fn is_redact_active(&self) -> Result<bool, Error> {
        Ok(self.0.ctx.try_get(&INTERACTION)?.is_some_and(|i| i.active_tool_id() == "redact"))
    }

    /// Mark the current text selection (per page) and clear it. False
    /// without create authority, a selection plugin, or a selection.
    pub fn queue_current_selection(&self) -> Promise<bool> {
        let this = self.clone();
        Promise::spawn(async move {
            if !this.0.anno.can_create() {
                return Ok(false);
            }
            let Some(selection) = this.0.ctx.try_get(&SELECTION)? else { return Ok(false) };
            if !selection.has_selection() {
                return Ok(false);
            }
            let snapshot = selection.snapshot();
            for page in snapshot.pages {
                if page.segments.is_empty() {
                    continue;
                }
                // The selected text frame goes all the way into `/QuadPoints`.
                let quads = page.segments.into_iter().map(|s| s.quad).collect();
                this.0.anno.create_markup("redact", page.pon, quads, Some("redact"));
            }
            selection.clear();
            Ok(true)
        })
    }

    /// `await Promise.all(pons().map((pon) => anno.ensurePage(pon)))`.
    pub fn prepare_pending(&self) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            let loads = this
                .pons()
                .into_iter()
                .map(|pon| {
                    this.0.anno.ensure_page(pon);
                    Promise::resolved(())
                })
                .collect();
            all(loads).await?;
            Ok(())
        })
    }

    /// Pending marks over the loaded annotation state.
    pub fn get_pending(&self) -> Vec<RedactionPendingItem> {
        self.collect_pending()
    }

    pub fn pending_count(&self) -> usize {
        self.collect_pending().len()
    }

    /// How many other annotations the given pending marks (default: all)
    /// would destroy.
    pub fn estimate_collateral(&self, ids: Option<&[String]>) -> u32 {
        estimate_collateral(&self.pages(), &self.list(), ids)
    }

    /// Edit a mark's label, always carrying its current `/DA` styling.
    pub fn set_label(&self, r: AnnotationRef, patch: RedactionLabelPatch) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            let current = this.0.anno.get(&r).filter(is_redact).ok_or_else(|| {
                Error::new("[redaction] setLabel target is not a redact annotation")
            })?;
            let mut body = Map::new();
            body.insert("subtype".into(), json!("redact"));
            match &patch.overlay_text {
                Nullable::Undefined => {}
                Nullable::Null => {
                    body.insert("overlayText".into(), Value::Null);
                }
                Nullable::Value(text) => {
                    body.insert("overlayText".into(), json!(text));
                }
            }
            if let Some(repeat) = patch.repeat {
                body.insert("repeat".into(), json!(repeat));
            }
            for key in ["fontFamily", "fontSize", "fontColor", "textAlign"] {
                if let Some(value) = current.get(key).filter(|v| !is_undefined(v)) {
                    body.insert(key.into(), value.clone());
                }
            }
            this.0.anno.update(r, Value::Object(body)).await
        })
    }

    /// Apply specific pending marks (annotations scope). Irreversible.
    pub fn apply(&self, ids: &[String]) -> Promise<RedactionApplyResult> {
        let this = self.clone();
        let wanted: HashSet<String> = ids.iter().cloned().collect();
        Promise::spawn(async move {
            let refs: Vec<Value> = this
                .collect_pending()
                .into_iter()
                .filter(|p| wanted.contains(&p.id))
                .map(|p| p.ref_)
                .collect();
            if refs.is_empty() {
                return Err(Error::new("[redaction] no matching pending redactions"));
            }
            adopt(this.run_apply(json!({ "kind": "annotations", "refs": refs }))).await
        })
    }

    /// Apply every redaction in the document (pages scope, including marks
    /// on pages this client never loaded). Irreversible.
    pub fn apply_all(&self) -> Promise<RedactionApplyResult> {
        let this = self.clone();
        Promise::spawn(async move {
            let page_object_numbers = this.pons();
            if page_object_numbers.is_empty() {
                return Err(Error::new("[redaction] document has no pages"));
            }
            adopt(
                this.run_apply(
                    json!({ "kind": "pages", "pageObjectNumbers": page_object_numbers }),
                ),
            )
            .await
        })
    }

    /// Fires after any confirmed apply, own or a remote collaborator's.
    /// Returns `() => set.delete(cb)`.
    pub fn on_applied(&self, callback: AppliedCallback) -> Rc<dyn Fn() -> bool> {
        self.0.applied.add(callback.clone());
        let applied = self.0.applied.clone();
        Rc::new(move || applied.delete(&callback))
    }
}

/// `return promise` from an `async` function: the resolve-thenable job,
/// then the `.then` reaction one turn after `promise` settles.
async fn adopt<T: Clone + 'static>(promise: Promise<T>) -> Result<T, Error> {
    rt::tick().await;
    promise.await
}

#[cfg(test)]
mod tests {
    //! The listener set's `for...of` rules under re-entry. No vector reaches
    //! them: a scripted `$fn` callback can't call an unsubscribe or
    //! `onApplied` while `fireApplied` is running.

    use super::*;

    fn logger(log: &Rc<RefCell<Vec<&'static str>>>, name: &'static str) -> AppliedCallback {
        let log = log.clone();
        Rc::new(move |_| {
            log.borrow_mut().push(name);
            Ok(())
        })
    }

    fn live(set: &AppliedCallbacks) -> usize {
        set.0.entries.borrow().len()
    }

    fn fire(set: &AppliedCallbacks) {
        set.for_each(|cb| cb(&Value::Null).unwrap());
    }

    #[test]
    fn adding_twice_keeps_one_entry_and_delete_compacts() {
        let set = AppliedCallbacks::default();
        let log = Rc::new(RefCell::new(Vec::new()));
        let a = logger(&log, "a");
        set.add(a.clone());
        set.add(a.clone());
        assert_eq!(live(&set), 1);
        assert!(set.delete(&a));
        assert_eq!(live(&set), 0);
        assert!(!set.delete(&a));
    }

    #[test]
    fn a_listener_deleting_itself_mid_pass_does_not_skip_the_next() {
        let set = AppliedCallbacks::default();
        let log = Rc::new(RefCell::new(Vec::new()));
        let slot: Rc<RefCell<Option<AppliedCallback>>> = Rc::default();
        let (s, l, me) = (set.clone(), log.clone(), slot.clone());
        let a: AppliedCallback = Rc::new(move |_| {
            l.borrow_mut().push("a");
            let own = me.borrow().clone().expect("own callback");
            assert!(s.delete(&own));
            Ok(())
        });
        *slot.borrow_mut() = Some(a.clone());
        set.add(a);
        set.add(logger(&log, "b"));
        fire(&set);
        assert_eq!(*log.borrow(), ["a", "b"]);
        assert_eq!(live(&set), 1);
        fire(&set);
        assert_eq!(*log.borrow(), ["a", "b", "b"]);
    }

    #[test]
    fn deleted_before_its_turn_is_skipped_and_added_mid_pass_runs() {
        let set = AppliedCallbacks::default();
        let log = Rc::new(RefCell::new(Vec::new()));
        let b = logger(&log, "b");
        let c = logger(&log, "c");
        let (s, l, b2, c2) = (set.clone(), log.clone(), b.clone(), c.clone());
        set.add(Rc::new(move |_| {
            l.borrow_mut().push("a");
            s.delete(&b2);
            s.add(c2.clone());
            Ok(())
        }));
        set.add(b);
        fire(&set);
        assert_eq!(*log.borrow(), ["a", "c"]);
        assert_eq!(live(&set), 2);
        assert_eq!(set.0.walking.get(), 0);
    }
}
