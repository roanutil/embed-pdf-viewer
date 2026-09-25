//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, and the function
//! table the reducer suite calls.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::{AnyPlugin, AnyToken, Error, is_undefined};
use plugin_conformance::{FunctionTable, Harness, HarnessPlugin, Outcome, scrub};
use serde_json::{Value, json};

use crate::types::{OpenSurfaceOptions, ShellAction, ShellState};
use crate::{SHELL, ShellCapability, initial_shell_state, shell_plugin, shell_reducer};

pub struct Shell;

/// The TypeScript object literal's member order.
const MEMBERS: &[&str] = &[
    "isOpen",
    "surfaceProps",
    "openSurfaces",
    "openMenus",
    "isMenuOpen",
    "open",
    "close",
    "toggle",
    "openMenu",
    "closeMenu",
    "toggleMenu",
    "closeAllMenus",
];

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v))
}

fn options(value: Option<&Value>) -> Option<OpenSurfaceOptions> {
    let value = scrub(value?);
    Some(OpenSurfaceOptions {
        exclusive: value.get("exclusive").and_then(Value::as_str).map(str::to_string),
        props: value.get("props").and_then(Value::as_object).cloned(),
    })
}

impl HarnessPlugin for Shell {
    fn factories(&self) -> Vec<&'static str> {
        vec!["shell"]
    }

    fn build(&self, _factory: &str, _config: &Value, _h: &Harness) -> Result<AnyPlugin, Error> {
        Ok(shell_plugin())
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![SHELL.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        _h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<ShellCapability>()?;
        let id = arg(&args, 0).and_then(Value::as_str).unwrap_or_default();
        Some(match method {
            "isOpen" => Outcome::Returned(json!(cap.is_open(id))),
            "surfaceProps" => match cap.surface_props(id) {
                Some(props) => Outcome::Returned(Value::Object(props)),
                None => Outcome::undefined(),
            },
            "openSurfaces" => Outcome::Returned(json!(cap.open_surfaces())),
            "openMenus" => Outcome::Returned(json!(cap.open_menus())),
            "isMenuOpen" => Outcome::Returned(json!(cap.is_menu_open(id))),
            "open" | "close" | "toggle" | "openMenu" | "closeMenu" | "toggleMenu"
            | "closeAllMenus" => {
                match method {
                    "open" => cap.open(id, options(arg(&args, 1))),
                    "close" => cap.close(id),
                    "toggle" => cap.toggle(id, options(arg(&args, 1))),
                    "openMenu" => cap.open_menu(id),
                    "closeMenu" => cap.close_menu(id),
                    "toggleMenu" => cap.toggle_menu(id),
                    _ => cap.close_all_menus(),
                }
                Outcome::undefined()
            }
            other => {
                Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
            }
        })
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<ShellCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

fn bad(what: &str, e: impl std::fmt::Display) -> Error {
    Error::new(format!("[shell harness] {what}: {e}"))
}

/// The reducer with its reference identity made visible. An action no
/// variant decodes is the TypeScript's `default: return state`.
fn f_reducer(args: &[Value]) -> Result<Value, Error> {
    let state: ShellState = serde_json::from_value(scrub(arg(args, 0).unwrap_or(&Value::Null)))
        .map_err(|e| bad("state", e))?;
    let action: Option<ShellAction> =
        arg(args, 1).and_then(|a| serde_json::from_value(scrub(a)).ok());
    let next = action.and_then(|action| shell_reducer(&state, &action));
    let shown =
        serde_json::to_value(next.as_ref().unwrap_or(&state)).map_err(|e| bad("state", e))?;
    Ok(json!({ "same": next.is_none(), "next": shown }))
}

fn f_initial(_: &[Value]) -> Result<Value, Error> {
    serde_json::to_value(initial_shell_state()).map_err(|e| bad("state", e))
}

pub fn functions() -> FunctionTable {
    vec![("shellReducer", f_reducer), ("initialShellState", f_initial)]
}
