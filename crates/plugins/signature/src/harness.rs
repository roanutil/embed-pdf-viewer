//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, plus the function
//! table. It mirrors the shims in
//! `tooling/plugin-conformance/src/suites/signature-plugin.ts`:
//!
//! - `console.error` lines starting `[signature]` log as the callback of
//!   that name while a signature factory is current (the plugin writes them
//!   through [`crate::console`]; the sink is put back when the case's kernel
//!   is dropped);
//! - `config.signer` is the `key` setting, `{kind: 'cms' | 'raw', answer,
//!   ...}`, whose `sign` logs as `signer.sign`; `thunk: true` wraps it in a
//!   function that logs `signer.resolve`. `config.trust` is `{anchors}`,
//!   logged as `trust.anchors`. A per-call key is a `$fn` or a port object
//!   in the call's arguments;
//! - `config.shape: 'controller'` builds `signature.plugin.ts`'s definition
//!   over `createSignatureController` with no registered settings;
//! - the `signature-annotation` factory is the suite's probe over the
//!   annotation plugin's `stamps.arm` and `stamps.isArmed`.

use std::any::Any;
use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::bytes::Bytes;
use epdf_kernel::js::to_value;
use epdf_kernel::rt::Promise;
use epdf_kernel::{
    AnyPlugin, AnyToken, Error, Token, Unsubscribe, define, is_undefined, undefined,
};
use epdf_plugin_annotation::write::stamps::StampInput;
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};
use epdf_signature_core::{
    CmsSignRequest, CmsSigner, Digest, RawAlgorithm, RawSigner, SignerPort, TrustPort,
    ValidationTime,
};
use plugin_conformance::{Answer, Callback, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::SignatureCapability;
use crate::model::{SignatureState, set_busy, set_target, set_verdicts};
use crate::services::settings::PortTable;
use crate::{
    blank_page_pdf, initial_signature_state, same_field_ref, signature_plugin,
    signature_plugin_with,
};

/// The adapter.
pub struct Signature;

/// The composed api's member order (`composeApi` over the settings, the
/// areas' slices, then the authority twins and the events).
pub const MEMBERS: &[&str] = &[
    "getSettings",
    "updateSettings",
    "resetSettings",
    "onSettingsChanged",
    "getSnapshot",
    "getStatus",
    "listSignatures",
    "listUnsignedFields",
    "getSignature",
    "listVerdicts",
    "getVerdict",
    "getProtection",
    "getPending",
    "isBusy",
    "getMode",
    "getTarget",
    "refresh",
    "validate",
    "validateField",
    "analyzeChanges",
    "readRevision",
    "setTarget",
    "requestInspection",
    "sign",
    "prepareSignature",
    "completeSignature",
    "cancelPending",
    "fillField",
    "clearField",
    "placeMark",
    "canSign",
    "canFill",
    "canCertify",
    "canReadRevision",
    "onSigned",
    "onFilled",
    "onCleared",
    "onValidated",
    "onProtectionChanged",
    "onInvalidationPredicted",
    "onTargetChanged",
    "onSignRequested",
    "onInspectionRequested",
];

fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| !v.is_null() && !is_undefined(v))
}

fn defined(value: Option<&Value>) -> Option<Value> {
    value.filter(|v| !is_undefined(v)).cloned()
}

fn bytes(value: &Value) -> Vec<u8> {
    Bytes::from_json(value).map(|b| b.0).unwrap_or_default()
}

fn answer(answer: Answer) -> Promise<Value> {
    match answer {
        Answer::Returns(value) => Promise::resolved(value),
        Answer::Throws(error) => Promise::rejected(error),
        Answer::Promise(promise) => promise,
    }
}

// ── the console ───────────────────────────────────────────────────────────────

/// The case's console sink, put back as it was when the case's kernel
/// (which holds the plugin, and with it this guard) is dropped.
struct ConsoleGuard(Option<crate::console::ConsoleSink>);

impl Drop for ConsoleGuard {
    fn drop(&mut self) {
        crate::console::set_sink(self.0.take());
    }
}

fn install_console(h: &Harness) -> Rc<ConsoleGuard> {
    let log = h.clone();
    let previous = crate::console::set_sink(Some(Rc::new(move |message: &str, error: &Error| {
        log.log("console.error", vec![json!(message), error.to_canonical()]);
    })));
    Rc::new(ConsoleGuard(previous))
}

// ── ports ─────────────────────────────────────────────────────────────────────

fn port_from(sign: Callback, value: &Value) -> Result<SignerPort, Error> {
    if value.get("kind").and_then(Value::as_str) == Some("cms") {
        return Ok(SignerPort::Cms(CmsSigner {
            sign: Rc::new(move |request: CmsSignRequest| {
                answer(sign.call(vec![request.to_json()])).map(|v| Ok(bytes(&v)))
            }),
        }));
    }
    Ok(SignerPort::Raw(RawSigner {
        certificate_chain: value
            .get("certificateChain")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(bytes)
            .collect(),
        algorithm: RawAlgorithm::from_name(
            value.get("algorithm").and_then(Value::as_str).unwrap_or_default(),
        )
        .ok_or_else(|| Error::new("[signature harness] unknown raw algorithm"))?,
        hash: Digest::from_name(value.get("hash").and_then(Value::as_str).unwrap_or_default())
            .ok_or_else(|| Error::new("[signature harness] unknown hash"))?,
        sign: Rc::new(move |data: Vec<u8>| {
            answer(sign.call(vec![Bytes(data).to_json()])).map(|v| Ok(bytes(&v)))
        }),
    }))
}

/// A key in a call's arguments: a port object whose `sign` is a `$fn`, or
/// a `$fn` that answers one.
fn call_key(value: &Value, h: &Harness) -> Result<SignatureKey, Error> {
    if value.get("$fn").is_some() {
        let resolve = h.callback(value);
        let h = h.clone();
        return Ok(SignatureKey::Thunk(Rc::new(move || {
            let h = h.clone();
            answer(resolve.call(Vec::new())).map(move |port| {
                let sign = h.callback(port.get("sign").unwrap_or(&Value::Null));
                port_from(sign, &port)
            })
        })));
    }
    port_from(h.callback(value.get("sign").unwrap_or(&Value::Null)), value).map(SignatureKey::Port)
}

/// `config.signer`: `{kind, answer, ...}`, `thunk` for a function.
fn config_key(spec: &Value, h: &Harness) -> Result<SignatureKey, Error> {
    let mut sign = json!({ "$fn": "signer.sign" });
    if let (Some(Value::Object(answer)), Value::Object(out)) = (spec.get("answer"), &mut sign) {
        for (k, v) in answer {
            out.insert(k.clone(), v.clone());
        }
    }
    let port = port_from(h.callback(&sign), spec)?;
    if spec.get("thunk").and_then(Value::as_bool) != Some(true) {
        return Ok(SignatureKey::Port(port));
    }
    let h = h.clone();
    Ok(SignatureKey::Thunk(Rc::new(move || {
        h.log("signer.resolve", Vec::new());
        Promise::resolved(port.clone())
    })))
}

struct ScriptedTrust(Callback);

impl TrustPort for ScriptedTrust {
    fn anchors(&self) -> Promise<Vec<Vec<u8>>> {
        answer(self.0.call(Vec::new()))
            .map(|v| Ok(v.as_array().map(|a| a.iter().map(bytes).collect()).unwrap_or_default()))
    }
}

/// `configOf(spec, env)`: only the settings the spec names.
fn config_of(spec: &Value, h: &Harness) -> Result<SignatureConfig, Error> {
    let trust = present(spec.get("trust")).map(|t| {
        let mut anchors = json!({ "$fn": "trust.anchors" });
        if let (Some(Value::Object(answer)), Value::Object(out)) = (t.get("anchors"), &mut anchors)
        {
            for (k, v) in answer {
                out.insert(k.clone(), v.clone());
            }
        }
        Some(Rc::new(ScriptedTrust(h.callback(&anchors))) as Rc<dyn TrustPort>)
    });
    Ok(SignatureConfig {
        mode: defined(spec.get("mode")).map(|m| m.as_str().and_then(SignatureMode::from_name)),
        allow_certify: spec.get("allowCertify").and_then(Value::as_bool),
        key: present(spec.get("signer")).map(|s| config_key(s, h)).transpose()?.map(Some),
        trust,
    })
}

// ── arguments ─────────────────────────────────────────────────────────────────

fn mark_of(value: Option<&Value>) -> Option<Mark> {
    let value = present(value)?;
    if let Some(asset_id) = value.get("assetId") {
        return Some(Mark::Asset {
            asset_id: match asset_id {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            },
        });
    }
    Some(Mark::Source { source: value.get("source").cloned().unwrap_or_else(undefined) })
}

fn sign_input(value: &Value, h: &Harness) -> Result<SignFieldInput, Error> {
    Ok(SignFieldInput {
        field: value.get("field").cloned().unwrap_or_else(undefined),
        mark: mark_of(value.get("mark")),
        key: present(value.get("key")).map(|k| call_key(k, h)).transpose()?,
        signer: defined(value.get("signer")),
        appearance: defined(value.get("appearance")),
        certify: defined(value.get("certify")),
        lock: defined(value.get("lock")),
    })
}

fn prepare_input(value: &Value) -> PrepareSignatureInput {
    PrepareSignatureInput {
        field: value.get("field").cloned().unwrap_or_else(undefined),
        mark: mark_of(value.get("mark")),
        appearance: defined(value.get("appearance")),
        signer: defined(value.get("signer")),
        certify: defined(value.get("certify")),
        lock: defined(value.get("lock")),
        sub_filter: defined(value.get("subFilter")),
        digest: defined(value.get("digest")),
    }
}

fn validate_options(value: Option<&Value>) -> SignatureValidateOptions {
    let Some(opts) = present(value) else { return SignatureValidateOptions::default() };
    let at = present(opts.get("at")).map(|at| match at.get("kind").and_then(Value::as_str) {
        Some("timestamp") => ValidationTime::Timestamp {
            gen_time: at.get("genTime").and_then(Value::as_f64).unwrap_or(f64::NAN),
        },
        _ => ValidationTime::Now,
    });
    SignatureValidateOptions {
        at,
        until: present(opts.get("until")).and_then(Value::as_str).map(str::to_string),
        signal: None,
    }
}

fn optional(value: Option<Value>) -> Value {
    value.unwrap_or(Value::Null)
}

fn unsubscribe_ref(h: &Harness, un: Unsubscribe) -> Outcome {
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        un.call();
        Outcome::undefined()
    })))
}

/// An event hook: subscribe a scripted callback, return its unsubscribe.
fn hook<E: Serialize + 'static>(
    h: &Harness,
    spec: Option<&Value>,
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &E| match callback.call(vec![to_value(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    unsubscribe_ref(h, off)
}

fn value_promise<T: Clone + 'static>(
    promise: Promise<T>,
    map: impl Fn(&T) -> Value + 'static,
) -> Outcome {
    Outcome::Promise(promise.map(move |v| Ok(map(&v))))
}

fn none() -> OperationOptions {
    OperationOptions::default()
}

fn call_signature(
    cap: &SignatureCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    let arg = |i: usize| args.get(i).filter(|v| !is_undefined(v));
    let field = || arg(0).cloned().unwrap_or_else(undefined);
    Ok(match method {
        "getSettings" => Outcome::Returned((*cap.settings_json()).clone()),
        "resetSettings" => {
            cap.reset_settings();
            Outcome::undefined()
        }
        "getSnapshot" => Outcome::Returned(optional(cap.get_snapshot())),
        "getStatus" => Outcome::Returned(json!(cap.get_status().as_str())),
        "listSignatures" => Outcome::Returned(Value::Array((*cap.list_signatures()).clone())),
        "listUnsignedFields" => {
            Outcome::Returned(Value::Array((*cap.list_unsigned_fields()).clone()))
        }
        "getSignature" => Outcome::Returned(optional(cap.get_signature(&field()))),
        "listVerdicts" => {
            Outcome::Returned(cap.list_verdicts().map_or(Value::Null, |v| (*v).clone()))
        }
        "getVerdict" => Outcome::Returned(optional(cap.get_verdict(&field()))),
        "getProtection" => Outcome::Returned(optional(cap.get_protection())),
        "getPending" => Outcome::Returned(cap.get_pending_json()),
        "isBusy" => Outcome::Returned(json!(cap.is_busy())),
        "getMode" => Outcome::Returned(json!(cap.get_mode().as_str())),
        "getTarget" => Outcome::Returned(optional(cap.get_target())),
        "refresh" => value_promise(cap.refresh(none()), |s| s.clone().unwrap_or(Value::Null)),
        "validate" => value_promise(cap.validate(validate_options(arg(0))), |v| (**v).clone()),
        "validateField" => Outcome::Promise(cap.validate_field(field(), validate_options(arg(1)))),
        "analyzeChanges" => Outcome::Promise(cap.analyze_changes(field(), none())),
        "readRevision" => Outcome::Promise(cap.read_revision(field(), none())),
        "setTarget" => {
            cap.set_target(present(arg(0)).cloned());
            Outcome::undefined()
        }
        "requestInspection" => {
            cap.request_inspection(field());
            Outcome::undefined()
        }
        "sign" => Outcome::Promise(cap.sign(sign_input(&field(), h)?, none())),
        "prepareSignature" => {
            Outcome::Promise(cap.prepare_signature(prepare_input(&field()), none()))
        }
        "completeSignature" => {
            let id = arg(0).map(|v| crate::js::template(Some(v))).unwrap_or_default();
            Outcome::Promise(cap.complete_signature(
                id,
                arg(1).map(bytes).unwrap_or_default(),
                none(),
            ))
        }
        "cancelPending" => Outcome::promise_unit(cap.cancel_pending(none())),
        "fillField" => {
            let mark = mark_of(arg(1)).ok_or_else(|| Error::new("[signature harness] no mark"))?;
            Outcome::promise_unit(cap.fill_field(field(), mark, none()))
        }
        "clearField" => Outcome::promise_unit(cap.clear_field(field(), none())),
        "placeMark" => {
            let mark = mark_of(arg(0)).ok_or_else(|| Error::new("[signature harness] no mark"))?;
            let target = arg(1).cloned().unwrap_or(Value::Null);
            let target = match target.get("field") {
                Some(field) => PlaceTarget::Field(field.clone()),
                None => PlaceTarget::Free(target),
            };
            value_promise(cap.place_mark(mark, target, none()), PlaceMarkResult::to_json)
        }
        "canSign" => Outcome::Returned(json!(cap.can_sign(&field()))),
        "canFill" => Outcome::Returned(json!(cap.can_fill(&field()))),
        "canCertify" => Outcome::Returned(json!(cap.can_certify())),
        "canReadRevision" => Outcome::Returned(json!(cap.can_read_revision())),
        "onSettingsChanged" => {
            let callback = h.callback(arg(0).unwrap_or(&Value::Null));
            let off = cap.on_settings_changed_json(move |settings, changed| {
                match callback.call(vec![json!({ "settings": settings, "changed": changed })]) {
                    Answer::Throws(error) => Err(error),
                    _ => Ok(()),
                }
            });
            unsubscribe_ref(h, off)
        }
        "onSigned" => hook(h, arg(0), |f| cap.on_signed(f)),
        "onFilled" => hook(h, arg(0), |f| cap.on_filled(f)),
        "onCleared" => hook(h, arg(0), |f| cap.on_cleared(f)),
        "onValidated" => hook(h, arg(0), |f| cap.on_validated(f)),
        "onProtectionChanged" => hook(h, arg(0), |f| cap.on_protection_changed(f)),
        "onInvalidationPredicted" => hook(h, arg(0), |f| cap.on_invalidation_predicted(f)),
        "onTargetChanged" => hook(h, arg(0), |f| cap.on_target_changed(f)),
        "onSignRequested" => hook(h, arg(0), |f| cap.on_sign_requested(f)),
        "onInspectionRequested" => hook(h, arg(0), |f| cap.on_inspection_requested(f)),
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    })
}

// ── the annotation probe ──────────────────────────────────────────────────────

/// The suite's `signature-annotation` probe: `stamps.arm` and
/// `stamps.isArmed` over the document's annotation plugin.
#[derive(Clone)]
pub struct AnnotationProbe(AnnotationCapability);

/// The probe's token (`env.token('signature-annotation')`).
pub static ANNOTATION_PROBE: LazyLock<Token<AnnotationProbe>> =
    LazyLock::new(|| Token::new("signature-annotation"));

fn annotation_probe() -> AnyPlugin {
    define::<Value, Value>("signature-annotation")
        .document_scoped()
        .requires(ANNOTATION.any())
        .create(&ANNOTATION_PROBE, |cx| Ok((AnnotationProbe(cx.get(&ANNOTATION)?), None)))
        .build()
}

fn call_probe(probe: &AnnotationProbe, method: &str, args: &[Value]) -> Outcome {
    match method {
        "arm" => {
            let input = StampInput::read(args.first().unwrap_or(&Value::Null));
            Outcome::promise_unit(probe.0.stamps_arm(input, None))
        }
        "isArmed" => Outcome::Returned(json!(probe.0.stamps_is_armed())),
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    }
}

impl HarnessPlugin for Signature {
    fn factories(&self) -> Vec<&'static str> {
        vec!["signature", "signature-annotation"]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        if factory == "signature-annotation" {
            return Ok(annotation_probe());
        }
        let console = install_console(h);
        let plugin = match present(Some(config)) {
            None => signature_plugin(SignatureConfig::default()),
            Some(spec) if spec.get("shape").is_none() => signature_plugin(config_of(spec, h)?),
            // `signature.plugin.ts`'s definition over the unit-test shape:
            // the settings from the definition, none registered.
            Some(_) => signature_plugin_with(Rc::new(PortTable::default()), None),
        };
        Ok(keep_alive(plugin, console))
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![SIGNATURE.any().clone(), ANNOTATION_PROBE.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        if let Some(probe) = capability.downcast_ref::<AnnotationProbe>() {
            return Some(call_probe(probe, method, &args));
        }
        let cap = capability.downcast_ref::<SignatureCapability>()?;
        Some(call_signature(cap, method, &args, h).unwrap_or_else(Outcome::Threw))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<SignatureCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

/// The plugin, holding `guard` for as long as the kernel holds it.
fn keep_alive(plugin: AnyPlugin, guard: Rc<ConsoleGuard>) -> AnyPlugin {
    let mut def = Rc::try_unwrap(plugin).unwrap_or_else(|_| unreachable!("a fresh plugin"));
    if let Some(create) = def.create.take() {
        def.create = Some(Rc::new(move |cx| {
            let _held = &guard;
            create(cx)
        }));
    }
    Rc::new(def)
}

// ── functions ─────────────────────────────────────────────────────────────────

fn initial_signature_state_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&initial_signature_state()))
}

fn state_arg(value: &Value) -> Result<Rc<SignatureState>, Error> {
    SignatureState::from_json(value).map(Rc::new)
}

fn member(value: Option<&Value>) -> Option<Rc<Value>> {
    value.filter(|v| !v.is_null() && !is_undefined(v)).cloned().map(Rc::new)
}

fn set_verdicts_fn(a: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&*set_verdicts(&state_arg(&a[0])?, member(a.get(1)))))
}

fn set_busy_fn(a: &[Value]) -> Result<Value, Error> {
    let busy = a.get(1).is_some_and(crate::js::truthy);
    Ok(to_value(&*set_busy(&state_arg(&a[0])?, busy)))
}

/// The suite's `setTarget(target, field)`: the transition from a state
/// whose target is `target`, and whether it changed.
fn set_target_fn(a: &[Value]) -> Result<Value, Error> {
    let before = Rc::new(SignatureState { target: member(a.first()), ..initial_signature_state() });
    let state = set_target(&before, member(a.get(1)));
    Ok(json!({ "changed": !Rc::ptr_eq(&state, &before), "state": to_value(&*state) }))
}

fn blank_page_pdf_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(Bytes(blank_page_pdf()).to_json())
}

fn mark_role_of_fn(a: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&mark_role_of(a[0].get("name").and_then(Value::as_str).unwrap_or_default())))
}

fn same_ref_fn(a: &[Value]) -> Result<Value, Error> {
    Ok(json!(same_field_ref(&a[0], &a[1])))
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![
        ("initialSignatureState", initial_signature_state_fn),
        ("setVerdicts", set_verdicts_fn),
        ("setBusy", set_busy_fn),
        ("setTarget", set_target_fn),
        ("blankPagePdf", blank_page_pdf_fn),
        ("markRoleOf", mark_role_of_fn),
        ("sameRef", same_ref_fn),
    ]
}
