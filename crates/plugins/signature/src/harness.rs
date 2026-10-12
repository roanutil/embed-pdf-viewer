//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, plus the function
//! table. It mirrors the shims in
//! `tooling/plugin-conformance/src/suites/signature-plugin.ts`:
//!
//! - `console.error` from this plugin logs as the callback of that name
//!   (`SignatureHost::console_error`);
//! - `config.signer` is `{kind: 'cms' | 'raw', answer, ...}`, whose `sign`
//!   logs as `signer.sign`, and `thunk: true` wraps it in a function that
//!   logs `signer.resolve`; `config.trust` is `{anchors}`, logged as
//!   `trust.anchors`; a per-call signer is a `$fn` or a port object in the
//!   call's arguments;
//! - `config.shape` builds the api with `createSignatureCapability` or
//!   `createSignatureController`, without a config.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::bytes::Bytes;
use epdf_kernel::js::{from_value, to_value};
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::Promise;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Unsubscribe, define, is_undefined, undefined};
use epdf_plugin_annotation::{ANNOTATION, StampPlacement};
use epdf_plugin_form::FORM;
use epdf_plugin_interaction::INTERACTION;
use epdf_plugin_stamp::STAMP;
use epdf_signature_core::{
    CmsSignRequest, CmsSigner, Digest, RawAlgorithm, RawSigner, SignerPort, TrustPort,
    ValidationTime,
};
use plugin_conformance::{Answer, Callback, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::{
    SignatureCapability, create_signature_capability, create_signature_controller,
};
use crate::model::{SignatureAction, SignatureState};
use crate::sync::hydration::ValidateRequest;
use crate::{
    blank_page_pdf, initial_signature_state, is_target, same_ref, signature_plugin,
    signature_reducer,
};

/// The adapter.
pub struct Signature;

/// The composed api's member order (`composeApi` over the controller's
/// slices).
pub const MEMBERS: &[&str] = &[
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
    "abortPending",
    "fillField",
    "clearField",
    "placeMark",
    "canSign",
    "canFill",
    "canCertify",
    "onSigned",
    "onFilled",
    "onCleared",
    "onValidated",
    "onProtectionChanged",
    "onInvalidating",
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

/// A signer in a call's arguments: a port object whose `sign` is a `$fn`,
/// or a `$fn` that answers one.
fn call_signer(value: &Value, h: &Harness) -> Result<SignerSource, Error> {
    if value.get("$fn").is_some() {
        let resolve = h.callback(value);
        let h = h.clone();
        return Ok(SignerSource::Thunk(Rc::new(move || {
            let h = h.clone();
            answer(resolve.call(Vec::new())).map(move |port| {
                let sign = h.callback(port.get("sign").unwrap_or(&Value::Null));
                port_from(sign, &port)
            })
        })));
    }
    port_from(h.callback(value.get("sign").unwrap_or(&Value::Null)), value).map(SignerSource::Port)
}

/// `config.signer`: `{kind, answer, ...}`, `thunk` for a function.
fn config_signer(spec: &Value, h: &Harness) -> Result<SignerSource, Error> {
    let mut sign = json!({ "$fn": "signer.sign" });
    if let (Some(Value::Object(answer)), Value::Object(out)) = (spec.get("answer"), &mut sign) {
        for (k, v) in answer {
            out.insert(k.clone(), v.clone());
        }
    }
    let port = port_from(h.callback(&sign), spec)?;
    if spec.get("thunk").and_then(Value::as_bool) != Some(true) {
        return Ok(SignerSource::Port(port));
    }
    let h = h.clone();
    Ok(SignerSource::Thunk(Rc::new(move || {
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

fn host(h: &Harness) -> SignatureHost {
    let h = h.clone();
    SignatureHost {
        console_error: Some(Rc::new(move |message: &str, error: &Error| {
            h.log("console.error", vec![json!(message), error.to_canonical()]);
        })),
    }
}

fn config_of(spec: &Value, h: &Harness) -> Result<SignatureConfig, Error> {
    let mode = match spec.get("mode").and_then(Value::as_str) {
        Some("sign") => Some(SignatureMode::Sign),
        Some("visual") => Some(SignatureMode::Visual),
        Some("ask") => Some(SignatureMode::Ask),
        _ => None,
    };
    let trust = present(spec.get("trust")).map(|t| {
        let mut anchors = json!({ "$fn": "trust.anchors" });
        if let (Some(Value::Object(answer)), Value::Object(out)) = (t.get("anchors"), &mut anchors)
        {
            for (k, v) in answer {
                out.insert(k.clone(), v.clone());
            }
        }
        Rc::new(ScriptedTrust(h.callback(&anchors))) as Rc<dyn TrustPort>
    });
    Ok(SignatureConfig {
        mode,
        signer: present(spec.get("signer")).map(|s| config_signer(s, h)).transpose()?,
        trust,
        allow_certify: spec.get("allowCertify").and_then(Value::as_bool),
        host: host(h),
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
        signer: present(value.get("signer")).map(|s| call_signer(s, h)).transpose()?,
        attribution: defined(value.get("attribution")),
        appearance: present(value.get("appearance")).cloned(),
        certify: defined(value.get("certify")),
        lock: defined(value.get("lock")),
    })
}

fn prepare_input(value: &Value) -> PrepareSignatureInput {
    PrepareSignatureInput {
        field: value.get("field").cloned().unwrap_or_else(undefined),
        mark: mark_of(value.get("mark")),
        appearance: present(value.get("appearance")).cloned(),
        attribution: defined(value.get("attribution")),
        certify: defined(value.get("certify")),
        lock: defined(value.get("lock")),
        sub_filter: defined(value.get("subFilter")),
        digest: defined(value.get("digest")),
    }
}

fn validate_request(value: Option<&Value>) -> ValidateRequest {
    let Some(opts) = present(value) else { return ValidateRequest::default() };
    let at = present(opts.get("at")).map(|at| match at.get("kind").and_then(Value::as_str) {
        Some("timestamp") => ValidationTime::Timestamp {
            gen_time: at.get("genTime").and_then(Value::as_f64).unwrap_or(f64::NAN),
        },
        _ => ValidationTime::Now,
    });
    ValidateRequest {
        at,
        until: present(opts.get("until")).and_then(Value::as_str).map(str::to_string),
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

fn call_signature(
    cap: &SignatureCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    let arg = |i: usize| args.get(i).filter(|v| !is_undefined(v));
    let field = || arg(0).cloned().unwrap_or_else(undefined);
    Ok(match method {
        "getSnapshot" => Outcome::Returned(optional(cap.get_snapshot())),
        "getStatus" => Outcome::Returned(to_value(&cap.get_status())),
        "listSignatures" => Outcome::Returned(Value::Array(cap.list_signatures())),
        "listUnsignedFields" => Outcome::Returned(Value::Array(cap.list_unsigned_fields())),
        "getSignature" => Outcome::Returned(optional(cap.get_signature(&field()))),
        "listVerdicts" => Outcome::Returned(optional(cap.list_verdicts())),
        "getVerdict" => Outcome::Returned(optional(cap.get_verdict(&field()))),
        "getProtection" => Outcome::Returned(optional(cap.get_protection())),
        "getPending" => Outcome::Returned(optional(cap.get_pending().map(|p| to_value(&p)))),
        "isBusy" => Outcome::Returned(json!(cap.is_busy())),
        "getMode" => Outcome::Returned(to_value(&cap.get_mode())),
        "getTarget" => Outcome::Returned(optional(cap.get_target())),
        "refresh" => value_promise(cap.refresh(), |s| s.clone().unwrap_or(Value::Null)),
        "validate" => Outcome::Promise(cap.validate(validate_request(arg(0)))),
        "validateField" => Outcome::Promise(cap.validate_field(field(), validate_request(arg(1)))),
        "analyzeChanges" => match cap.analyze_changes(field()) {
            Ok(promise) => Outcome::Promise(promise),
            Err(error) => Outcome::Threw(error),
        },
        "readRevision" => match cap.read_revision(&field()) {
            Ok(promise) => Outcome::Promise(promise),
            Err(error) => Outcome::Threw(error),
        },
        "setTarget" => {
            cap.set_target(present(arg(0)).cloned());
            Outcome::undefined()
        }
        "requestInspection" => {
            cap.request_inspection(field());
            Outcome::undefined()
        }
        "sign" => Outcome::Promise(cap.sign(sign_input(&field(), h)?)),
        "prepareSignature" => Outcome::Promise(cap.prepare_signature(prepare_input(&field()))),
        "completeSignature" => {
            let id = arg(0).and_then(Value::as_str).unwrap_or_default().to_string();
            Outcome::Promise(cap.complete_signature(id, arg(1).map(bytes).unwrap_or_default()))
        }
        "abortPending" => Outcome::promise_unit(cap.abort_pending()),
        "fillField" => {
            let mark = mark_of(arg(1)).ok_or_else(|| Error::new("[signature harness] no mark"))?;
            Outcome::promise_unit(cap.fill_field(field(), mark))
        }
        "clearField" => Outcome::promise_unit(cap.clear_field(field())),
        "placeMark" => {
            let mark = mark_of(arg(0)).ok_or_else(|| Error::new("[signature harness] no mark"))?;
            let target = arg(1).cloned().unwrap_or(Value::Null);
            let target = match target.get("field") {
                Some(field) => PlaceTarget::Field(field.clone()),
                None => PlaceTarget::Free(from_value::<StampPlacement>(&target)?),
            };
            value_promise(cap.place_mark(mark, target), PlaceMarkResult::to_json)
        }
        "canSign" => Outcome::Returned(json!(cap.can_sign())),
        "canFill" => Outcome::Returned(json!(cap.can_fill())),
        "canCertify" => Outcome::Returned(json!(cap.can_certify())),
        "onSigned" => hook(h, arg(0), |f| cap.on_signed(f)),
        "onFilled" => hook(h, arg(0), |f| cap.on_filled(f)),
        "onCleared" => hook(h, arg(0), |f| cap.on_cleared(f)),
        "onValidated" => hook(h, arg(0), |f| cap.on_validated(f)),
        "onProtectionChanged" => hook(h, arg(0), |f| cap.on_protection_changed(f)),
        "onInvalidating" => hook(h, arg(0), |f| cap.on_invalidating(f)),
        "onTargetChanged" => hook(h, arg(0), |f| cap.on_target_changed(f)),
        "onSignRequested" => hook(h, arg(0), |f| cap.on_sign_requested(f)),
        "onInspectionRequested" => hook(h, arg(0), |f| cap.on_inspection_requested(f)),
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    })
}

impl HarnessPlugin for Signature {
    fn factories(&self) -> Vec<&'static str> {
        vec!["signature"]
    }

    fn build(&self, _factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        let shape = config.get("shape").and_then(Value::as_str).map(str::to_string);
        let Some(shape) = shape else {
            return Ok(signature_plugin(config_of(config, h)?));
        };
        // `signature.plugin.ts`'s definition over the unit-test shapes, with
        // no config (only the host the console shim needs).
        let config = SignatureConfig { host: host(h), ..SignatureConfig::default() };
        Ok(define::<SignatureState, SignatureAction>("signature")
            .document_scoped()
            .requires(FORM.any())
            .optional(INTERACTION.any())
            .optional(STAMP.any())
            .optional(ANNOTATION.any())
            .initial_state(initial_signature_state)
            .reduce(signature_reducer)
            .create(&SIGNATURE, move |cx| {
                if shape == "capability" {
                    Ok((create_signature_capability(cx, config.clone())?, None))
                } else {
                    let (api, connect) = create_signature_controller(cx, config.clone())?;
                    Ok((api, Some(connect as ConnectFn)))
                }
            })
            .build())
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![SIGNATURE.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<SignatureCapability>()?;
        Some(call_signature(cap, method, &args, h).unwrap_or_else(Outcome::Threw))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<SignatureCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── functions ─────────────────────────────────────────────────────────────────

fn initial_signature_state_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&initial_signature_state()))
}

/// `{changed: next !== state, state: next}`. An action outside the union is
/// the reducer's `return state`.
fn signature_reducer_fn(a: &[Value]) -> Result<Value, Error> {
    let state: SignatureState = from_value(&a[0])?;
    let next = match from_value::<SignatureAction>(&a[1]) {
        Ok(action) => signature_reducer(&state, &action),
        Err(_) => None,
    };
    Ok(json!({ "changed": next.is_some(), "state": to_value(next.as_ref().unwrap_or(&state)) }))
}

fn blank_page_pdf_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(Bytes(blank_page_pdf()).to_json())
}

fn mark_role_of_fn(a: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&mark_role_of(a[0].get("name").and_then(Value::as_str).unwrap_or_default())))
}

fn is_target_fn(a: &[Value]) -> Result<Value, Error> {
    Ok(json!(is_target(present(a.first()), &a[1])))
}

fn same_ref_fn(a: &[Value]) -> Result<Value, Error> {
    Ok(json!(same_ref(&a[0], &a[1])))
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![
        ("initialSignatureState", initial_signature_state_fn),
        ("signatureReducer", signature_reducer_fn),
        ("blankPagePdf", blank_page_pdf_fn),
        ("markRoleOf", mark_role_of_fn),
        ("isTarget", is_target_fn),
        ("sameRef", same_ref_fn),
    ]
}
