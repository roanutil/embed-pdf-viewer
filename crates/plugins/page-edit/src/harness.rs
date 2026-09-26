//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::bytes::Bytes;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::{AnyPlugin, AnyToken, Error, is_undefined};
use plugin_conformance::{Harness, HarnessPlugin, Outcome, scrub};
use serde_json::Value;

use crate::contract::{InsertBlankOptions, InsertFromBytesOptions, PagePlacement, PdfSize};
use crate::{PAGE_EDIT, PageEditCapability, page_edit_plugin};

pub struct PageEdit;

/// The TypeScript object literal's member order.
const MEMBERS: &[&str] = &[
    "canEdit",
    "rotateBy",
    "setRotation",
    "move",
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

fn page_ref(value: Option<&Value>) -> PageRef {
    value.and_then(PageRef::from_json).unwrap_or(PageRef::new(0))
}

fn refs(value: Option<&Value>) -> Vec<PageRef> {
    value
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|v| page_ref(Some(v))).collect())
        .unwrap_or_default()
}

/// `!placement || placement === 'end'`, then `'index' in placement`, then
/// `'after' in placement`, else `before`.
fn placement(value: Option<&Value>) -> Option<PagePlacement> {
    let placement = value?;
    if placement.as_str() == Some("end") {
        return Some(PagePlacement::End);
    }
    if let Some(index) = placement.get("index") {
        return Some(PagePlacement::Index(number(Some(index))));
    }
    match placement.get("after") {
        Some(after) => Some(PagePlacement::After(page_ref(Some(after)))),
        None => Some(PagePlacement::Before(page_ref(placement.get("before")))),
    }
}

fn option_placement(options: Option<&Value>) -> Option<PagePlacement> {
    placement(options?.get("placement").filter(|p| !is_undefined(p)))
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
        Some(match method {
            "canEdit" => Outcome::from_result(cap.can_edit().map(Value::Bool)),
            "rotateBy" => {
                Outcome::Promise(cap.rotate_by(refs(arg(&args, 0)), number(arg(&args, 1))))
            }
            "setRotation" => {
                Outcome::Promise(cap.set_rotation(refs(arg(&args, 0)), number(arg(&args, 1))))
            }
            "move" => {
                Outcome::Promise(cap.move_pages(refs(arg(&args, 0)), placement(arg(&args, 1))))
            }
            "delete" => Outcome::Promise(cap.delete(refs(arg(&args, 0)))),
            "insertBlank" => {
                let options = arg(&args, 0);
                let size = options
                    .and_then(|o| o.get("size"))
                    .and_then(|s| serde_json::from_value::<PdfSize>(s.clone()).ok());
                let count = options.and_then(|o| o.get("count")).map(|c| number(Some(c)));
                Outcome::Promise(cap.insert_blank(InsertBlankOptions {
                    size,
                    count,
                    placement: option_placement(options),
                }))
            }
            "insertFromBytes" => {
                let bytes = arg(&args, 0).and_then(Bytes::from_json).unwrap_or_default();
                let options = arg(&args, 1);
                let page_indexes = options
                    .and_then(|o| o.get("pageIndexes"))
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|i| number(Some(i))).collect());
                Outcome::Promise(cap.insert_from_bytes(
                    bytes,
                    InsertFromBytesOptions { page_indexes, placement: option_placement(options) },
                ))
            }
            "insertFromDocument" => Outcome::Promise(cap.insert_from_document(
                arg(&args, 0).and_then(Value::as_str).unwrap_or_default().to_string(),
                refs(arg(&args, 1)),
                option_placement(arg(&args, 2)),
            )),
            "duplicate" => Outcome::Promise(
                cap.duplicate(refs(arg(&args, 0)), option_placement(arg(&args, 1))),
            ),
            "extract" => Outcome::promise(cap.extract(refs(arg(&args, 0)))),
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
