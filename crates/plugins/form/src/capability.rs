//! `capability.ts`: the form shell. The pure `update` runs here, the model
//! it returns is dispatched to the store, and engine calls happen around it.
//!
//! The TypeScript's public capability and its host lens (`resetFormAction`,
//! `resolveSubmitDataset`, `commitScriptFormEffects`) are one runtime
//! object; so is [`FormCapability`]. The host members say so in their docs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_acrojs::{ScriptDiagnostic, ScriptDiagnosticCode};
use epdf_annotation_core::AnnotationRef as TypedAnnotationRef;
use epdf_geometry::Rect;
use epdf_geometry::js::{max, min};
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::serial_queue::SerialQueue;
use epdf_kernel::{Ctx, Error, undefined};
use epdf_plugin_actions::{
    ACTIONS, ActionContext, ActionDiagnostic, ActionOrigin, ActionResultStatus, ActionSource,
    ActionSubmitRequest, ActionTrigger, ActionsCapability, HoverEvents, HoverPump, HoverTarget,
    PdfActionTargetRef, PdfAnnotationEventKind, ScriptPhase, ScriptSurfaceResult, SubmitIntent,
    create_hover_pump,
};
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};
use serde_json::{Value, json};

use crate::core::fill_items::{
    FillItem, fill_item_for_widget as core_fill_item_for_widget, fill_items as core_fill_items,
};
use crate::core::model::{
    Box, FieldKey, Model, Msg, field_by_key, field_for_widget as core_field_for_widget, number_key,
    update,
};
use crate::field_selection::{build_submit_entries, resolve_field_selection};
use crate::promise::{adopt, adopted, await_engine};
use crate::scripting::{
    FormScriptingController, FormScriptingControllerOptions, TransactionBody, TransactionPort,
    create_form_scripting_controller,
};
use crate::types::{
    FormAction, FormCommitResult, FormCommitStatus, FormHost, FormPluginConfig, FormState,
    PlaceFieldInput, PlacedField, WidgetActivationResult,
};
use crate::value::{
    f64_of, get, member, num, number, str_of, strict_eq, string_of, string_to_number, truthy,
    utf16_slice,
};

type FillCache = HashMap<String, (f64, bool, Rc<Vec<FillItem>>)>;
type FillItemCache = HashMap<String, (f64, bool, Option<Rc<FillItem>>)>;

pub(crate) struct Inner {
    ctx: Ctx<FormState, FormAction>,
    host: FormHost,
    queue: SerialQueue,
    actions_host: Option<ActionsCapability>,
    scripting: Option<FormScriptingController>,
    annotation_host: Option<AnnotationCapability>,
    refresh_promise: RefCell<Option<Promise<()>>>,
    geom_loading: RefCell<Vec<f64>>,
    fill_cache: RefCell<FillCache>,
    fill_item_cache: RefCell<FillItemCache>,
    widget_pump: RefCell<Option<HoverPump>>,
}

/// `FormCapability` and `FormHostCapability`: the field plane.
#[derive(Clone)]
pub struct FormCapability(pub(crate) Rc<Inner>);

/// `refKeyOf(key)`: a field key back as an engine ref.
pub fn ref_key_of(key: &str) -> Value {
    let rest = utf16_slice(key, 4.0, None);
    if key.starts_with("obj:") {
        json!({ "kind": "objectNumber", "fieldObjectNumber": number(string_to_number(&rest)) })
    } else {
        json!({ "kind": "fqn", "name": rest })
    }
}

/// `toBox(rect, crop)`: PDF user space (y-up) to a crop-relative content
/// box (y-down).
fn to_box(rect: &Value, crop: &Value) -> Box {
    let r = |k: &str| f64_of(rect, k);
    let c = |k: &str| f64_of(crop, k);
    Box {
        x: r("left") - c("left"),
        y: c("top") - r("top"),
        width: r("right") - r("left"),
        height: r("top") - r("bottom"),
    }
}

/// `toPdfRect(box, crop)`, the inverse of [`to_box`].
fn to_pdf_rect(bx: &Box, crop: &Value) -> Value {
    let c = |k: &str| f64_of(crop, k);
    json!({
        "left": number(bx.x + c("left")),
        "top": number(c("top") - bx.y),
        "right": number(bx.x + c("left") + bx.width),
        "bottom": number(c("top") - bx.y - bx.height),
    })
}

/// `sameAnnotationRef(left, right)`.
fn same_annotation_ref(left: &Value, right: &Value) -> bool {
    let m = |v: &Value, k: &str| get(v, k).cloned();
    let eq = |k: &str| strict_eq(m(left, k).as_ref(), m(right, k).as_ref());
    if !eq("kind") || !eq("pageObjectNumber") {
        return false;
    }
    let kind = (str_of(left, "kind"), str_of(right, "kind"));
    match kind {
        (Some("objectNumber"), Some("objectNumber")) => eq("annotObjectNumber"),
        (Some("nm"), Some("nm")) => eq("nm"),
        _ => kind == (Some("index"), Some("index")) && eq("index") && eq("revision"),
    }
}

fn not_scripted() -> FormCommitResult {
    FormCommitResult::bare(FormCommitStatus::Unchanged, false)
}

fn failed_batch(effects: &[Value], code: &str, message: &str) -> Value {
    let results: Vec<Value> = (0..effects.len())
        .map(|index| {
            json!({
                "index": index,
                "status": "failed",
                "fields": [],
                "changedWidgets": [],
                "error": { "code": code, "message": message },
            })
        })
        .collect();
    json!({ "results": results, "changedWidgets": [], "meta": null })
}

/// The page number an engine page call takes.
fn page_u32(pon: f64) -> u32 {
    pon as u32
}

/// `createFormCapability(ctx, options)`.
pub fn create_form_capability(
    ctx: Ctx<FormState, FormAction>,
    config: &FormPluginConfig,
) -> Result<FormCapability, Error> {
    // Scripting rides the actions plugin's realm: the transaction port's
    // presence is the "JavaScript is on" signal.
    let actions_host = ctx.try_get(&ACTIONS)?;
    let scripting = match (&actions_host, ctx.doc()) {
        (Some(actions), Some(doc)) if actions.has_script_transaction() => {
            let port_actions = actions.clone();
            let transaction: TransactionPort = Rc::new(move |body: TransactionBody| {
                port_actions
                    .script_transaction(body)
                    .unwrap_or_else(|| Promise::rejected(Error::new("no script transaction")))
            });
            let document_ctx = ctx.clone();
            Some(create_form_scripting_controller(FormScriptingControllerOptions::Transaction {
                doc,
                document: Rc::new(move || document_ctx.document()),
                transaction,
                budget: None,
            }))
        }
        _ => None,
    };
    if let Some(scripting) = &scripting {
        let scripting = scripting.clone();
        ctx.cleanup_fn(move || scripting.dispose());
    }
    let annotation_host = ctx.try_get(&ANNOTATION)?;
    let capability = FormCapability(Rc::new(Inner {
        ctx,
        host: config.host.clone(),
        queue: SerialQueue::new(),
        actions_host,
        scripting,
        annotation_host,
        refresh_promise: RefCell::new(None),
        geom_loading: RefCell::new(Vec::new()),
        fill_cache: RefCell::new(HashMap::new()),
        fill_item_cache: RefCell::new(HashMap::new()),
        widget_pump: RefCell::new(None),
    }));
    capability.refresh_with(false).ignore();
    Ok(capability)
}

impl FormCapability {
    fn ctx(&self) -> &Ctx<FormState, FormAction> {
        &self.0.ctx
    }

    fn doc(&self) -> Option<Rc<dyn DocumentHandle>> {
        self.ctx().doc()
    }

    fn require_doc(&self) -> Result<Rc<dyn DocumentHandle>, Error> {
        self.doc().ok_or_else(|| Error::new("no document"))
    }

    fn model(&self) -> Rc<Model> {
        self.ctx().state().model.clone()
    }

    fn apply(&self, msg: Msg) {
        let model = update(&self.model(), &msg);
        self.ctx().dispatch(FormAction::SetModel { model });
    }

    /// Wildcard-aware authority for the twins, the hydration gate, the fused
    /// projection and the write gate.
    fn can(&self, capability: &str) -> bool {
        self.doc().is_some_and(|doc| doc.security().allows(capability))
    }

    fn crop_of(&self, pon: f64) -> Option<Value> {
        let meta = self.ctx().document()?;
        let page =
            meta.pages.as_array()?.iter().find(|p| num(p, "pageObjectNumber") == Some(pon))?;
        get(page, "boxes").and_then(|b| get(b, "crop")).cloned()
    }

    fn warn(&self, message: &str, error: &Error) {
        if let Some(sink) = &self.0.host.console_warn {
            sink(message, error);
        }
    }

    pub(crate) fn console_error(&self, message: &str, error: &Error) {
        if let Some(sink) = &self.0.host.console_error {
            sink(message, error);
        }
    }

    /// `surfaceViaActions(result, origin)`: every script surface flows
    /// through the actions plugin's one port, boot requests first. `Err` is
    /// an adapter that threw.
    fn surface_via_actions(
        &self,
        result: &FormCommitResult,
        origin: ActionOrigin,
    ) -> Result<(), Error> {
        let Some(actions) = &self.0.actions_host else { return Ok(()) };
        for phase in [ScriptPhase::Boot, ScriptPhase::User] {
            let ui_effects: Vec<_> = result
                .ui_effects
                .iter()
                .filter(|effect| effect.phase == phase)
                .map(|effect| effect.effect.clone())
                .collect();
            if ui_effects.is_empty() && phase == ScriptPhase::Boot {
                continue;
            }
            let user = phase == ScriptPhase::User;
            actions.surface_script_result(&ScriptSurfaceResult {
                ui_effects,
                diagnostics: if user { result.diagnostics.clone() } else { Vec::new() },
                error: if user { result.error.clone() } else { None },
                origin,
                phase,
            })?;
        }
        Ok(())
    }

    // ── snapshot loading ──────────────────────────────────────────────────────

    /// `refresh()`: re-read the form from the engine.
    pub fn refresh(&self) -> Promise<()> {
        self.refresh_with(false)
    }

    /// `refresh(force)`: a refresh while one is in flight awaits it and,
    /// unless forced, returns.
    pub fn refresh_with(&self, force: bool) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            let Some(doc) = this.doc() else { return Ok(()) };
            if !this.can("doc.forms.read") {
                return Ok(());
            }
            let in_flight = this.0.refresh_promise.borrow().clone();
            if let Some(in_flight) = in_flight {
                let _ = in_flight.await;
                if !force {
                    return Ok(());
                }
            }
            let chain = this.load_snapshot(doc);
            *this.0.refresh_promise.borrow_mut() = Some(chain.clone());
            adopt(chain).await
        })
    }

    /// `doc.forms.list().then(apply).catch(warn).finally(clear)`.
    fn load_snapshot(&self, doc: Rc<dyn DocumentHandle>) -> Promise<()> {
        let call = doc.call("forms.list", None, Vec::new());
        let this = self.clone();
        let applied: Promise<()> = Promise::spawn(async move {
            let snapshot = call.await?;
            this.apply(Msg::Snapshot { snapshot });
            Ok(())
        });
        let this = self.clone();
        let caught: Promise<()> = Promise::spawn(async move {
            if let Err(error) = applied.await {
                this.warn("[form] form snapshot failed to load", &error);
            }
            Ok(())
        });
        let this = self.clone();
        Promise::spawn(async move {
            let result = caught.await;
            *this.0.refresh_promise.borrow_mut() = None;
            rt::tick().await;
            rt::tick().await;
            result
        })
    }

    /// `snapshot()`: null until the first load lands.
    pub fn snapshot(&self) -> Option<Value> {
        self.model().snapshot.clone()
    }

    // ── widget geometry ───────────────────────────────────────────────────────

    /// `ensureGeom(pon)`: load a page's widget geometry, once, lazily.
    pub fn ensure_geom(&self, pon: f64) {
        let Some(doc) = self.doc() else { return };
        if self.0.geom_loading.borrow().contains(&pon)
            || self.model().geom.contains_key(&number_key(pon))
        {
            return;
        }
        let Some(crop) = self.crop_of(pon) else { return };
        if doc.page_index(page_u32(pon)).is_err() {
            return;
        }
        self.0.geom_loading.borrow_mut().push(pon);
        let call = doc.call("page.annotations.list", Some(page_u32(pon)), Vec::new());
        let this = self.clone();
        let loaded: Promise<()> = Promise::spawn(async move {
            let list = call.await?;
            let mut boxes = std::collections::BTreeMap::new();
            for dto in get(&list, "annotations").and_then(Value::as_array).into_iter().flatten() {
                if str_of(dto, "subtype") != Some("widget") {
                    continue;
                }
                let r = get(dto, "ref").unwrap_or(&Value::Null);
                let object_number = if str_of(r, "kind") == Some("objectNumber") {
                    f64_of(r, "annotObjectNumber")
                } else {
                    0.0
                };
                if object_number > 0.0 {
                    boxes.insert(
                        number_key(object_number),
                        to_box(get(dto, "rect").unwrap_or(&Value::Null), &crop),
                    );
                }
            }
            this.apply(Msg::PageGeom { page_object_number: pon, boxes });
            Ok(())
        });
        let this = self.clone();
        rt::spawn(async move {
            let _ = loaded.await;
            this.0.geom_loading.borrow_mut().retain(|&p| p != pon);
        });
    }

    // ── the fill projection ───────────────────────────────────────────────────

    fn fuse_fill(item: Option<FillItem>, fillable: bool) -> Option<FillItem> {
        match item {
            Some(item) if !fillable && !item.disabled => Some(FillItem { disabled: true, ..item }),
            other => other,
        }
    }

    /// `fillItems(pon)`: one page's fill controls, the same `Rc` while the
    /// model and the fill grant are unchanged. Without `doc.forms.fill`
    /// every item is disabled.
    pub fn fill_items(&self, pon: f64) -> Rc<Vec<FillItem>> {
        let m = self.model();
        let fillable = self.can("doc.forms.fill");
        let key = number_key(pon);
        if let Some((seq, cached_fillable, items)) = self.0.fill_cache.borrow().get(&key)
            && *seq == m.seq
            && *cached_fillable == fillable
        {
            return items.clone();
        }
        let items: Rc<Vec<FillItem>> = Rc::new(
            core_fill_items(&m, pon)
                .into_iter()
                .filter_map(|item| Self::fuse_fill(Some(item), fillable))
                .collect(),
        );
        self.0.fill_cache.borrow_mut().insert(key, (m.seq, fillable, items.clone()));
        items
    }

    /// `fillItem(annotObjectNumber)`: one widget's fill control, the same
    /// `Rc` per model change.
    pub fn fill_item(&self, annot_object_number: f64) -> Option<Rc<FillItem>> {
        let m = self.model();
        let fillable = self.can("doc.forms.fill");
        let key = number_key(annot_object_number);
        if let Some((seq, cached_fillable, item)) = self.0.fill_item_cache.borrow().get(&key)
            && *seq == m.seq
            && *cached_fillable == fillable
        {
            return item.clone();
        }
        let item = Self::fuse_fill(core_fill_item_for_widget(&m, annot_object_number), fillable)
            .map(Rc::new);
        self.0.fill_item_cache.borrow_mut().insert(key, (m.seq, fillable, item.clone()));
        item
    }

    /// `field(key)`.
    pub fn field(&self, key: &str) -> Option<Value> {
        field_by_key(&self.model(), key)
    }

    /// `fieldForWidget(annotObjectNumber)`.
    pub fn field_for_widget(&self, annot_object_number: f64) -> Option<Value> {
        core_field_for_widget(&self.model(), annot_object_number)
    }

    // ── typed writes ──────────────────────────────────────────────────────────

    async fn commit_value_now(
        &self,
        field_ref: Value,
        value: Value,
    ) -> Result<FormCommitResult, Error> {
        let doc = self.require_doc()?;
        if let Some(scripting) = &self.0.scripting {
            let result = scripting.commit(field_ref, value).await?;
            self.surface_via_actions(&result, ActionOrigin::User)?;
            // A native partial or failed result can still have mutated state.
            if result.effects_result.is_some() {
                self.refresh_with(true).await?;
            }
            return Ok(result);
        }
        let result = await_engine(doc.call("forms.setValue", None, vec![field_ref, value])).await?;
        self.refresh_with(true).await?;
        let changed = get(&result, "changedWidgets").and_then(Value::as_array).map_or(0, Vec::len);
        Ok(FormCommitResult::bare(
            if changed > 0 { FormCommitStatus::Applied } else { FormCommitStatus::Unchanged },
            false,
        ))
    }

    fn write(&self, key: FieldKey, value: Value) -> Promise<()> {
        // The optimistic gate: refuse before the spinner and the queue.
        if !self.can("doc.forms.fill") {
            return Promise::rejected(Error::permission_denied(
                "doc.forms.fill",
                Some("form.fill"),
            ));
        }
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                if this.doc().is_none() {
                    return Ok(());
                }
                this.apply(Msg::WriteStart { key: key.clone() });
                match rt::returned(this.commit_value_now(ref_key_of(&key), value)).await {
                    Ok(result) => {
                        let rejected = matches!(
                            result.status,
                            FormCommitStatus::Rejected | FormCommitStatus::Failed
                        );
                        // A scripted no-op has no read-back to clear the spinner.
                        if rejected || (result.effects_result.is_none() && result.scripted) {
                            this.apply(Msg::WriteFailed { key });
                        }
                        Ok(())
                    }
                    Err(error) => {
                        this.apply(Msg::WriteFailed { key });
                        Err(error)
                    }
                }
            }))
        })
    }

    /// `setText(key, value)`: commit a text value.
    pub fn set_text(&self, key: FieldKey, value: Value) -> Promise<()> {
        self.write(key, json!({ "type": "text", "value": value }))
    }

    /// `toggle(key, onState)`: a checkbox or radio by its on-state; null
    /// clears the group.
    pub fn toggle(&self, key: FieldKey, on_state: Value) -> Promise<()> {
        self.write(key, json!({ "type": "toggle", "state": on_state }))
    }

    /// `choose(key, values)`: choice options by export value.
    pub fn choose(&self, key: FieldKey, values: Value) -> Promise<()> {
        self.write(key, json!({ "type": "choice", "values": values }))
    }

    /// `commitValue(ref, value)`: one value transaction, K/V/C/F included
    /// when scripting is on.
    pub fn commit_value(&self, field_ref: Value, value: Value) -> Promise<FormCommitResult> {
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move { this.commit_value_now(field_ref, value).await }))
        })
    }

    /// `reset(key)`: restore a field to its `/DV` default, through the same
    /// batch and recalculation a ResetForm action takes.
    pub fn reset(&self, key: FieldKey) -> Promise<()> {
        if !self.can("doc.forms.fill") {
            return Promise::rejected(Error::permission_denied(
                "doc.forms.fill",
                Some("form.reset"),
            ));
        }
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                let Some(doc) = this.doc() else { return Ok(()) };
                this.apply(Msg::WriteStart { key: key.clone() });
                let outcome: Result<(), Error> = async {
                    if doc.has("forms.applyEffects") {
                        let result = rt::returned(
                            this.apply_reset_batch(vec![ref_key_of(&key)], ActionOrigin::User),
                        )
                        .await?;
                        if result.status == FormCommitStatus::Failed {
                            let message = result
                                .effects_result
                                .as_ref()
                                .and_then(|r| get(r, "results"))
                                .and_then(Value::as_array)
                                .and_then(|entries| {
                                    entries.iter().find(|e| {
                                        matches!(str_of(e, "status"), Some("failed" | "rejected"))
                                    })
                                })
                                .and_then(|e| get(e, "error"))
                                .and_then(|e| get(e, "message"))
                                .filter(|m| !m.is_null())
                                .map(|m| string_of(Some(m)))
                                .unwrap_or_else(|| "reset failed".into());
                            return Err(Error::new(message));
                        }
                        match field_by_key(&this.model(), &key) {
                            Some(field) => this.apply(Msg::WriteDone { key: key.clone(), field }),
                            None => this.apply(Msg::WriteFailed { key: key.clone() }),
                        }
                    } else {
                        let result =
                            await_engine(doc.call("forms.reset", None, vec![ref_key_of(&key)]))
                                .await?;
                        let field = get(&result, "field").cloned().unwrap_or(Value::Null);
                        this.apply(Msg::WriteDone { key: key.clone(), field });
                    }
                    Ok(())
                }
                .await;
                if outcome.is_err() {
                    this.apply(Msg::WriteFailed { key });
                }
                outcome
            }))
        })
    }

    /// `setValue(ref, value)`: the raw engine door, then a forced re-read.
    pub fn set_value(&self, field_ref: Value, value: Value) -> Promise<Value> {
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                let doc = this.require_doc()?;
                let result =
                    await_engine(doc.call("forms.setValue", None, vec![field_ref, value])).await?;
                this.refresh_with(true).await?;
                Ok(result)
            }))
        })
    }

    // ── the reset core and the action-executor doors ─────────────────────────

    /// The shared reset core: one effects batch, a re-read, then the `/CO`
    /// recalculation when scripting is on. `origin` is kept through the
    /// recalculation's surfacing.
    async fn apply_reset_batch(
        &self,
        refs: Vec<Value>,
        origin: ActionOrigin,
    ) -> Result<FormCommitResult, Error> {
        let doc = self.require_doc()?;
        if !doc.has("forms.applyEffects") {
            let result = FormCommitResult {
                diagnostics: vec![ScriptDiagnostic {
                    code: ScriptDiagnosticCode::UnsupportedApi,
                    message: "this engine has no form-effects batch door".into(),
                }],
                ..not_scripted()
            };
            self.surface_via_actions(&result, origin)?;
            return Ok(result);
        }
        // Zero refs never reach the engine: `[]` with include resets nothing.
        if refs.is_empty() {
            return Ok(not_scripted());
        }
        let effects = json!([{ "kind": "reset", "refs": refs }]);
        let effects_result =
            await_engine(doc.call("forms.applyEffects", None, vec![effects])).await?;
        let reset_failed = get(&effects_result, "results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|entry| matches!(str_of(entry, "status"), Some("failed" | "rejected")));
        self.refresh_with(true).await?;
        if reset_failed {
            return Ok(FormCommitResult {
                effects_result: Some(effects_result),
                ..FormCommitResult::bare(FormCommitStatus::Failed, false)
            });
        }
        // Acrobat recalculates after a reset; boot rides along lazily.
        let mut recalc = None;
        if let Some(scripting) = &self.0.scripting {
            let result = scripting.recalculate().await?;
            self.surface_via_actions(&result, origin)?;
            if result.effects_result.is_some() {
                self.refresh_with(true).await?;
            }
            recalc = Some(result);
        }
        Ok(FormCommitResult {
            status: FormCommitStatus::Applied,
            scripted: recalc.is_some(),
            effects_result: Some(effects_result),
            ui_effects: recalc.as_ref().map(|r| r.ui_effects.clone()).unwrap_or_default(),
            diagnostics: recalc.as_ref().map(|r| r.diagnostics.clone()).unwrap_or_default(),
            error: recalc.and_then(|r| r.error),
        })
    }

    /// Host lens: `resetFormAction(fields, exclude, origin = 'user')`. One
    /// ResetForm action: the ISO selection over a fresh read, buttons and
    /// signatures skipped, one batch.
    pub fn reset_form_action(
        &self,
        targets: Option<Vec<PdfActionTargetRef>>,
        exclude: bool,
        origin: Option<ActionOrigin>,
    ) -> Promise<FormCommitResult> {
        let origin = origin.unwrap_or(ActionOrigin::User);
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                let doc = this.require_doc()?;
                let snapshot = await_engine(doc.call("forms.list", None, Vec::new())).await?;
                let fields: Vec<Value> =
                    get(&snapshot, "fields").and_then(Value::as_array).cloned().unwrap_or_default();
                let selection = resolve_field_selection(&fields, targets.as_deref(), exclude);
                let refs: Vec<Value> = selection
                    .selected
                    .iter()
                    .filter(|f| !matches!(str_of(f, "family"), Some("pushbutton" | "signature")))
                    .map(|f| get(f, "ref").cloned().unwrap_or_else(undefined))
                    .collect();
                adopted(this.apply_reset_batch(refs, origin)).await
            }))
        })
    }

    /// Host lens: `resolveSubmitDataset(intent, ctx, diagnose)`. A fresh
    /// read, never the cached model, then the pure ISO builder.
    pub fn resolve_submit_dataset(
        &self,
        intent: SubmitIntent,
        action_ctx: ActionContext,
        diagnose: Rc<dyn Fn(ActionDiagnostic)>,
    ) -> Promise<ActionSubmitRequest> {
        let this = self.clone();
        Promise::spawn(async move {
            let doc = this.require_doc()?;
            let snapshot = await_engine(doc.call("forms.list", None, Vec::new())).await?;
            let fields: Vec<Value> =
                get(&snapshot, "fields").and_then(Value::as_array).cloned().unwrap_or_default();
            let entries = build_submit_entries(&fields, &intent, &*diagnose);
            Ok(ActionSubmitRequest {
                url: intent.url.clone(),
                method: intent.method.clone(),
                format: intent.format.clone(),
                flags_raw: intent.flags_raw,
                char_set: intent.char_set.clone(),
                entries,
                origin: action_ctx.origin,
                event: action_ctx.event,
            })
        })
    }

    /// Host lens: `commitScriptFormEffects(effects)`, the document commit
    /// sink. Never throws: a refused batch or a missing batch door resolves
    /// as all-failed.
    pub fn commit_script_form_effects(&self, effects: Vec<Value>) -> Promise<Value> {
        let this = self.clone();
        Promise::spawn(async move {
            let doc = match this.doc() {
                Some(doc) if doc.has("forms.applyEffects") => doc,
                _ => {
                    return Ok(failed_batch(
                        &effects,
                        "NotSupported",
                        "no form-effects batch door",
                    ));
                }
            };
            let call = doc.call("forms.applyEffects", None, vec![Value::Array(effects.clone())]);
            let result = match await_engine(call).await {
                Ok(result) => result,
                Err(error) => return Ok(failed_batch(&effects, "Refused", &error.message)),
            };
            this.refresh_with(true).await?;
            if let Some(annotation) = &this.0.annotation_host {
                let mut pons: Vec<f64> = Vec::new();
                for widget in
                    get(&result, "changedWidgets").and_then(Value::as_array).into_iter().flatten()
                {
                    let pon = f64_of(widget, "pageObjectNumber");
                    if pon > 0.0 && !pons.contains(&pon) {
                        pons.push(pon);
                    }
                }
                for pon in pons {
                    annotation.reload_page(page_u32(pon)).await?;
                }
            }
            Ok(result)
        })
    }

    // ── widget activation and the widget event feed ──────────────────────────

    fn widget_source(key: &str, annotation_ref: &Value) -> ActionSource {
        ActionSource::Widget {
            field: ref_key_of(key),
            annotation: annotation_ref.clone(),
            pon: f64_of(annotation_ref, "pageObjectNumber"),
        }
    }

    /// The annotation plugin's committed DTO for a ref, when it has one.
    fn loaded(&self, annotation_ref: &Value) -> Option<Value> {
        let annotation = self.0.annotation_host.as_ref()?;
        let typed: TypedAnnotationRef = epdf_kernel::js::from_value(annotation_ref).ok()?;
        annotation.get(&typed)
    }

    async fn annotation_activation(&self, annotation_ref: &Value) -> Result<Option<Value>, Error> {
        let Some(doc) = self.doc() else { return Ok(None) };
        let activate = |annotation: &Value| {
            get(annotation, "actions")
                .and_then(|a| get(a, "activate"))
                .filter(|t| !t.is_null())
                .cloned()
        };
        if let Some(loaded) = self.loaded(annotation_ref)
            && str_of(&loaded, "subtype") == Some("widget")
        {
            return Ok(activate(&loaded));
        }
        let pon = page_u32(f64_of(annotation_ref, "pageObjectNumber"));
        doc.page_index(pon)?;
        let list = await_engine(doc.call("page.annotations.list", Some(pon), Vec::new())).await?;
        let found = get(&list, "annotations").and_then(Value::as_array).into_iter().flatten().find(
            |candidate| {
                same_annotation_ref(get(candidate, "ref").unwrap_or(&Value::Null), annotation_ref)
            },
        );
        Ok(found.filter(|a| str_of(a, "subtype") == Some("widget")).and_then(activate))
    }

    async fn activate_widget_now(
        &self,
        key: FieldKey,
        annotation_ref: Value,
    ) -> Result<FormCommitResult, Error> {
        self.require_doc()?;
        let Some(scripting) = &self.0.scripting else { return Ok(not_scripted()) };
        let action = rt::returned(self.annotation_activation(&annotation_ref)).await?;
        let Some(action) = action.filter(|a| truthy(Some(a))) else {
            return Ok(FormCommitResult::bare(FormCommitStatus::Unchanged, true));
        };
        let result = scripting.activate(ref_key_of(&key), action).await?;
        self.surface_via_actions(&result, ActionOrigin::User)?;
        if result.effects_result.is_some() {
            self.refresh_with(true).await?;
        }
        Ok(result)
    }

    /// `activateWidget(key, annotationRef)`: with the actions plugin the
    /// whole `/A` tree is dispatched there; only a tree-less result (inert,
    /// no steps, no diagnostics) falls back to the form path.
    pub fn activate_widget(
        &self,
        key: FieldKey,
        annotation_ref: Value,
    ) -> Promise<WidgetActivationResult> {
        let this = self.clone();
        Promise::spawn(async move {
            if let Some(actions) = this.ctx().try_get(&ACTIONS)? {
                let result = actions
                    .dispatch(ActionTrigger::Activate {
                        annotation_ref: annotation_ref.clone(),
                        pon: f64_of(&annotation_ref, "pageObjectNumber"),
                        source: Some(Self::widget_source(&key, &annotation_ref)),
                    })
                    .await?;
                let no_tree = result.status == ActionResultStatus::Inert
                    && result.steps.is_empty()
                    && result.diagnostics.is_empty();
                if !no_tree {
                    return Ok(WidgetActivationResult::Dispatched(result));
                }
            }
            let now = this.clone();
            let result = this
                .0
                .queue
                .run(move || {
                    Ok(Promise::spawn(
                        async move { now.activate_widget_now(key, annotation_ref).await },
                    ))
                })
                .await?;
            Ok(WidgetActivationResult::Form(result))
        })
    }

    fn widget_hover_pump(&self, actions: &ActionsCapability) -> HoverPump {
        let mut pump = self.0.widget_pump.borrow_mut();
        pump.get_or_insert_with(|| {
            let actions = actions.clone();
            create_hover_pump(move |trigger| actions.dispatch(trigger))
        })
        .clone()
    }

    fn widget_hover_flags(&self, annotation_ref: &Value) -> Option<(bool, bool)> {
        let loaded = self.loaded(annotation_ref)?;
        if str_of(&loaded, "subtype") != Some("widget") {
            return None;
        }
        let root = |event: &str| {
            truthy(get(&loaded, "actions").and_then(|a| get(a, event)).and_then(|t| get(t, "root")))
        };
        Some((root("cursorEnter"), root("cursorExit")))
    }

    /// `notifyWidgetEvent(key, ref, event)`: report one widget DOM event.
    /// With the actions plugin the matching `/AA` tree dispatches (hover
    /// through the shared pump); without it this is a no-op.
    pub fn notify_widget_event(
        &self,
        key: &str,
        annotation_ref: &Value,
        event: PdfAnnotationEventKind,
    ) -> Result<(), Error> {
        let Some(actions) = self.ctx().try_get(&ACTIONS)? else { return Ok(()) };
        let source = Self::widget_source(key, annotation_ref);
        let pon = f64_of(annotation_ref, "pageObjectNumber");
        match event {
            PdfAnnotationEventKind::CursorExit => self.widget_hover_pump(&actions).hover(None),
            PdfAnnotationEventKind::CursorEnter => {
                let flags = self.widget_hover_flags(annotation_ref);
                if flags == Some((false, false)) {
                    return Ok(());
                }
                self.widget_hover_pump(&actions).hover(Some(HoverTarget {
                    annotation_ref: annotation_ref.clone(),
                    pon,
                    source: Some(source),
                    events: flags
                        .map(|(enter, exit)| HoverEvents { enter: Some(enter), exit: Some(exit) }),
                }));
            }
            _ => actions
                .dispatch(ActionTrigger::Annotation {
                    event,
                    annotation_ref: annotation_ref.clone(),
                    pon,
                    source: Some(source),
                })
                .ignore(),
        }
        Ok(())
    }

    // ── interchange ───────────────────────────────────────────────────────────

    /// `exportData(format = 'xfdf')`.
    pub fn export_data(&self, format: Option<Value>) -> Promise<Value> {
        let this = self.clone();
        Promise::spawn(async move {
            let doc = this.require_doc()?;
            let format = format.unwrap_or_else(|| json!("xfdf"));
            adopt(doc.call("forms.exportData", None, vec![format])).await
        })
    }

    /// `importData(data, format)`: SETs the snapshot the engine answers.
    pub fn import_data(&self, data: Value, format: Option<Value>) -> Promise<Value> {
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                let doc = this.require_doc()?;
                let args = vec![data, format.unwrap_or_else(undefined)];
                let result = await_engine(doc.call("forms.importData", None, args)).await?;
                let snapshot = get(&result, "snapshot").cloned().unwrap_or(Value::Null);
                this.apply(Msg::Snapshot { snapshot });
                Ok(result)
            }))
        })
    }

    /// `repair(options)`.
    pub fn repair(&self, options: Option<Value>) -> Promise<Value> {
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                let doc = this.require_doc()?;
                let args = vec![options.unwrap_or_else(undefined)];
                let result = await_engine(doc.call("forms.repair", None, args)).await?;
                this.refresh_with(true).await?;
                Ok(result)
            }))
        })
    }

    // ── design mode ───────────────────────────────────────────────────────────

    fn nudge_annotations(&self, pons: &[f64]) {
        let Some(annotation) = &self.0.annotation_host else { return };
        let mut seen: Vec<f64> = Vec::new();
        for &pon in pons {
            if !seen.contains(&pon) {
                seen.push(pon);
                annotation.reload_page(page_u32(pon)).ignore();
            }
        }
    }

    /// `pageBox(pon)`: the page's content box `{0, 0, w, h}`.
    pub fn page_box(&self, pon: f64) -> Option<Rect> {
        let crop = self.crop_of(pon)?;
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: f64_of(&crop, "right") - f64_of(&crop, "left"),
            height: f64_of(&crop, "top") - f64_of(&crop, "bottom"),
        })
    }

    /// `autoName(family)`: `text_1`, `text_2`, ... against the current snapshot.
    fn auto_name(&self, family: &str) -> String {
        let model = self.model();
        let names: Vec<&str> = model
            .snapshot
            .as_ref()
            .and_then(|s| get(s, "fields"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|f| str_of(f, "name"))
            .collect();
        let mut n = 1u64;
        while names.contains(&format!("{family}_{n}").as_str()) {
            n += 1;
        }
        format!("{family}_{n}")
    }

    async fn place_field_now(&self, input: PlaceFieldInput) -> Result<PlacedField, Error> {
        let pon = input.page_object_number;
        let (Some(doc), Some(crop)) = (self.doc(), self.crop_of(pon)) else {
            return Err(Error::new("[form] placeField: document/page not ready"));
        };
        let page = self.page_box(pon).unwrap_or_default();
        let x = max(page.x, min(input.bx.x, page.width));
        let y = max(page.y, min(input.bx.y, page.height));
        let bx = Rect {
            x,
            y,
            width: max(0.0, min(input.bx.x + input.bx.width, page.width) - x),
            height: max(0.0, min(input.bx.y + input.bx.height, page.height) - y),
        };
        if bx.width < 1.0 || bx.height < 1.0 {
            return Err(Error::new(
                "[form] placeField: degenerate box (size the box before placing)",
            ));
        }
        let family = input.family.as_str();
        let name = self.auto_name(family);
        let mut placement =
            json!({ "pageObjectNumber": number(pon), "rect": to_pdf_rect(&bx, &crop) });
        if let Some(appearance) = input.appearance.filter(|a| truthy(Some(a))) {
            placement["appearance"] = appearance;
        }
        let draft = match family {
            "radio" => {
                let mut widget = placement;
                widget["onState"] = json!("option1");
                json!({ "family": family, "name": name, "widgets": [widget] })
            }
            "combobox" | "listbox" => json!({
                "family": family,
                "name": name,
                "widget": placement,
                "options": [
                    { "label": "Option 1", "value": "Option 1" },
                    { "label": "Option 2", "value": "Option 2" },
                ],
            }),
            _ => json!({ "family": family, "name": name, "widget": placement }),
        };
        let result = await_engine(doc.call("forms.createField", None, vec![draft])).await?;
        self.refresh_with(true).await?;
        self.apply(Msg::ClearGeom { page_object_number: Some(pon) });
        // Await the annotation-plane reload so the widget is selectable.
        if let Some(annotation) = &self.0.annotation_host {
            annotation.reload_page(page_u32(pon)).await?;
        }
        let field = get(&result, "field").cloned().unwrap_or_else(undefined);
        let widget = get(&field, "widgets")
            .and_then(Value::as_array)
            .and_then(|widgets| widgets.iter().find(|w| num(w, "pageObjectNumber") == Some(pon)))
            .cloned();
        Ok(PlacedField { field, widget })
    }

    /// `placeField(input)`: create a field with one styled widget at a
    /// content-space box, clamped to the page, auto-named.
    pub fn place_field(&self, input: PlaceFieldInput) -> Promise<PlacedField> {
        let this = self.clone();
        self.0
            .queue
            .run(move || Ok(Promise::spawn(async move { this.place_field_now(input).await })))
    }

    /// `updateField(key, patch)`.
    pub fn update_field(&self, key: FieldKey, patch: Value) -> Promise<()> {
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                let Some(doc) = this.doc() else { return Ok(()) };
                let call = doc.call("forms.updateField", None, vec![ref_key_of(&key), patch]);
                await_engine(call).await?;
                this.refresh_with(true).await
            }))
        })
    }

    /// `deleteField(key)`: the field and every widget of it.
    pub fn delete_field(&self, key: FieldKey) -> Promise<()> {
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                let Some(doc) = this.doc() else { return Ok(()) };
                let pons: Vec<f64> = field_by_key(&this.model(), &key)
                    .and_then(|f| get(&f, "widgets").and_then(Value::as_array).cloned())
                    .into_iter()
                    .flatten()
                    .map(|w| f64_of(&w, "pageObjectNumber"))
                    .filter(|&p| p > 0.0)
                    .collect();
                await_engine(doc.call("forms.deleteField", None, vec![ref_key_of(&key)])).await?;
                this.refresh_with(true).await?;
                let mut seen: Vec<f64> = Vec::new();
                for &pon in &pons {
                    if !seen.contains(&pon) {
                        seen.push(pon);
                        this.apply(Msg::ClearGeom { page_object_number: Some(pon) });
                    }
                }
                this.nudge_annotations(&pons);
                Ok(())
            }))
        })
    }

    /// `detachWidget(key, annotObjectNumber)`: the widget stays as an inert
    /// annotation; the field survives.
    pub fn detach_widget(&self, key: FieldKey, annot_object_number: f64) -> Promise<()> {
        let this = self.clone();
        self.0.queue.run(move || {
            Ok(Promise::spawn(async move {
                let Some(doc) = this.doc() else { return Ok(()) };
                let widget = field_by_key(&this.model(), &key).and_then(|f| {
                    get(&f, "widgets")
                        .and_then(Value::as_array)?
                        .iter()
                        .find(|w| num(w, "annotObjectNumber") == Some(annot_object_number))
                        .cloned()
                });
                let widget_pon = widget.as_ref().map(|w| member(w, "pageObjectNumber"));
                let target = json!({
                    "annotObjectNumber": number(annot_object_number),
                    "pageObjectNumber": match &widget_pon {
                        Some(pon) if !epdf_kernel::is_undefined(pon) && !pon.is_null() => pon.clone(),
                        _ => json!(0),
                    },
                });
                await_engine(doc.call("forms.detachWidget", None, vec![ref_key_of(&key), target]))
                    .await?;
                this.refresh_with(true).await?;
                if let Some(pon) = widget_pon.as_ref().and_then(Value::as_f64)
                    && pon > 0.0
                {
                    this.apply(Msg::ClearGeom { page_object_number: Some(pon) });
                    this.nudge_annotations(&[pon]);
                }
                Ok(())
            }))
        })
    }

    // ── the twins ─────────────────────────────────────────────────────────────

    /// `canRead()`: the model hydrates at all.
    pub fn can_read(&self) -> bool {
        self.can("doc.forms.read")
    }

    /// `canFill()`: value writes would succeed.
    pub fn can_fill(&self) -> bool {
        self.can("doc.forms.fill")
    }

    /// `canDesign()`: the field-design family would succeed.
    pub fn can_design(&self) -> bool {
        self.can("doc.forms.modify")
    }
}
