//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name
//! (`tooling/plugin-conformance/src/suites/page-edit.ts`).

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::bytes::Bytes;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Promise, is_undefined, undefined};
use plugin_conformance::{Harness, HarnessPlugin, Outcome, scrub};
use serde_json::{Value, json};

use crate::contract::{
    InsertBlankOptions, InsertFromBytesOptions, OperationOptions, PageArg, PageEditInsertResult,
    PagePosition, PagePositionOptions, PdfSize,
};
use crate::{PAGE_EDIT, PageEditCapability, page_edit_plugin};

pub struct PageEdit;

/// The TypeScript `api` object literal's member order.
const MEMBERS: &[&str] = &[
    "canEdit",
    "canExtract",
    "getSelected",
    "select",
    "rotateBy",
    "setRotation",
    "reorder",
    "delete",
    "insertBlank",
    "insertFromBytes",
    "insertFromDocument",
    "duplicate",
    "extract",
];

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v))
}

/// A canonical number: plain, or `{"$number": ...}`.
fn number(value: Option<&Value>) -> f64 {
    let Some(value) = value else { return f64::NAN };
    if let Some(n) = value.as_f64() {
        return n;
    }
    match value.get("$number").and_then(Value::as_str) {
        Some("Infinity") => f64::INFINITY,
        Some("-Infinity") => f64::NEG_INFINITY,
        _ => f64::NAN,
    }
}

fn pages(value: Option<&Value>) -> Vec<PageArg> {
    value
        .and_then(Value::as_array)
        .map(|list| list.iter().map(PageArg::from_json).collect())
        .unwrap_or_default()
}

/// `'start' | 'end' | {after} | {before}`.
fn position(value: Option<&Value>) -> Option<PagePosition> {
    let value = value?;
    match value.as_str() {
        Some("start") => return Some(PagePosition::Start),
        Some(_) => return Some(PagePosition::End),
        None => {}
    }
    Some(match value.get("after") {
        Some(after) => PagePosition::After(PageArg::from_json(after)),
        None => {
            PagePosition::Before(PageArg::from_json(value.get("before").unwrap_or(&Value::Null)))
        }
    })
}

fn option<'a>(options: Option<&'a Value>, member: &str) -> Option<&'a Value> {
    options?.get(member).filter(|v| !is_undefined(v))
}

/// A promise whose value prints as JSON, converted the moment it settles so
/// the conversion costs no turn.
fn as_json<T: Clone + 'static>(promise: Promise<T>, to_json: fn(T) -> Value) -> Outcome {
    let (out, resolver) = Promise::pending();
    promise.on_settle(move |result| resolver.settle(result.clone().map(to_json)));
    Outcome::Promise(out)
}

fn inserted(promise: Promise<PageEditInsertResult>) -> Outcome {
    as_json(promise, |result| result.to_json())
}

fn unit(promise: Promise<()>) -> Outcome {
    as_json(promise, |()| undefined())
}

impl HarnessPlugin for PageEdit {
    fn factories(&self) -> Vec<&'static str> {
        vec!["page-edit"]
    }

    fn build(&self, _factory: &str, _config: &Value, _h: &Harness) -> Result<AnyPlugin, Error> {
        Ok(page_edit_plugin())
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![PAGE_EDIT.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        _h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<PageEditCapability>()?;
        let args: Vec<Value> = args.iter().map(scrub).collect();
        let plain = OperationOptions::default;
        Some(match method {
            "canEdit" => Outcome::Returned(json!(cap.can_edit())),
            "canExtract" => Outcome::Returned(json!(cap.can_extract())),
            "getSelected" => Outcome::Returned(json!(*cap.get_selected())),
            "select" => {
                cap.select(&pages(arg(&args, 0)));
                Outcome::undefined()
            }
            "rotateBy" => unit(cap.rotate_by(pages(arg(&args, 0)), number(arg(&args, 1)), plain())),
            "setRotation" => {
                unit(cap.set_rotation(pages(arg(&args, 0)), number(arg(&args, 1)), plain()))
            }
            "reorder" => unit(cap.reorder(
                pages(arg(&args, 0)),
                position(arg(&args, 1)).unwrap_or(PagePosition::End),
                plain(),
            )),
            "delete" => unit(cap.delete(pages(arg(&args, 0)), plain())),
            "insertBlank" => {
                let options = arg(&args, 0);
                inserted(
                    cap.insert_blank(InsertBlankOptions {
                        position: position(option(options, "position")),
                        size: option(options, "size")
                            .and_then(|s| serde_json::from_value::<PdfSize>(s.clone()).ok()),
                        count: option(options, "count").map(|c| number(Some(c))),
                        signal: None,
                    }),
                )
            }
            "insertFromBytes" => {
                let bytes = arg(&args, 0).and_then(Bytes::from_json).unwrap_or_default();
                let options = arg(&args, 1);
                let page_indexes = option(options, "pageIndexes")
                    .and_then(Value::as_array)
                    .map(|list| list.iter().map(|i| number(Some(i))).collect());
                inserted(cap.insert_from_bytes(
                    bytes,
                    InsertFromBytesOptions {
                        page_indexes,
                        position: position(option(options, "position")),
                        signal: None,
                    },
                ))
            }
            "insertFromDocument" => inserted(cap.insert_from_document(
                arg(&args, 0).and_then(Value::as_str).unwrap_or_default().to_string(),
                pages(arg(&args, 1)),
                PagePositionOptions {
                    position: position(option(arg(&args, 2), "position")),
                    signal: None,
                },
            )),
            "duplicate" => inserted(cap.duplicate(
                pages(arg(&args, 0)),
                PagePositionOptions {
                    position: position(option(arg(&args, 1), "position")),
                    signal: None,
                },
            )),
            "extract" => {
                as_json(cap.extract(pages(arg(&args, 0)), plain()), |bytes| bytes.to_json())
            }
            other => {
                Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
            }
        })
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<PageEditCapability>()?;
        Some(MEMBERS.to_vec())
    }
}
