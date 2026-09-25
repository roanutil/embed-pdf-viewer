//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::bytes::Bytes;
use epdf_kernel::engine::EngineCall;
use epdf_kernel::{AnyPlugin, AnyToken, Error, is_undefined};
use plugin_conformance::{Harness, HarnessPlugin, Outcome, scrub};
use serde_json::{Value, json};

use crate::types::{AddBlankOptions, InsertOptions, PageObjectNumber, PagePlacement, PdfSize};
use crate::{PAGE_EDIT, PageEditCapability, page_edit_plugin};

pub struct PageEdit;

/// The TypeScript object literal's member order.
const MEMBERS: &[&str] =
    &["canEdit", "rotateBy", "setRotation", "move", "delete", "addBlank", "insert"];

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

fn pon(value: Option<&Value>) -> PageObjectNumber {
    value.and_then(Value::as_u64).unwrap_or_default() as PageObjectNumber
}

fn pons(value: Option<&Value>) -> Vec<PageObjectNumber> {
    value
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|v| pon(Some(v))).collect())
        .unwrap_or_default()
}

/// `'index' in placement`, then `'after' in placement`, else `before`.
fn placement(opts: Option<&Value>) -> Option<PagePlacement> {
    let placement = opts?.get("placement").filter(|p| !is_undefined(p))?;
    if let Some(index) = placement.get("index") {
        return Some(PagePlacement::Index(number(Some(index))));
    }
    match placement.get("after") {
        Some(after) => Some(PagePlacement::After(pon(Some(after)))),
        None => Some(PagePlacement::Before(pon(placement.get("before")))),
    }
}

fn engine(result: Result<EngineCall<Value>, Error>) -> Outcome {
    match result {
        Ok(promise) => Outcome::Promise(promise),
        Err(error) => Outcome::Threw(error),
    }
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
            "canEdit" => Outcome::Returned(json!(cap.can_edit())),
            "rotateBy" => engine(cap.rotate_by(pon(arg(&args, 0)), number(arg(&args, 1)))),
            "setRotation" => engine(cap.set_rotation(&pons(arg(&args, 0)), number(arg(&args, 1)))),
            "move" => engine(cap.move_pages(&pons(arg(&args, 0)), number(arg(&args, 1)))),
            "delete" => engine(cap.delete(&pons(arg(&args, 0)))),
            "addBlank" => {
                let opts = arg(&args, 0);
                let size = opts
                    .and_then(|o| o.get("size"))
                    .and_then(|s| serde_json::from_value::<PdfSize>(s.clone()).ok());
                let count = opts.and_then(|o| o.get("count")).map(|c| number(Some(c)));
                engine(cap.add_blank(AddBlankOptions { size, count, placement: placement(opts) }))
            }
            "insert" => {
                let bytes = arg(&args, 0).and_then(Bytes::from_json).unwrap_or_default();
                engine(cap.insert(&bytes, InsertOptions { placement: placement(arg(&args, 1)) }))
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
