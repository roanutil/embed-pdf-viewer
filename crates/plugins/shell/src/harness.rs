//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, and the function
//! table the reducer suite calls.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::{AnyPlugin, AnyToken, Error, is_undefined};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome, scrub};
use serde_json::{Value, json};

use crate::contract::{OpenSurfaceOptions, ShellSnapshot, SurfaceProps};
use crate::model::{ShellAction, ShellState};
use crate::{SHELL, ShellCapability, initial_shell_state, shell_plugin, shell_reducer};

pub struct Shell;

/// The TypeScript object literal's member order.
const MEMBERS: &[&str] = &[
    "isOpen",
    "getSurface",
    "listOpenSurfaces",
    "open",
    "close",
    "toggle",
    "updateSurfaceProps",
    "closeAll",
    "isMenuOpen",
    "listOpenMenus",
    "openMenu",
    "closeMenu",
    "toggleMenu",
    "closeAllMenus",
    "getSnapshot",
    "applySnapshot",
    "onSurfaceOpened",
    "onSurfaceClosed",
    "onMenuOpened",
    "onMenuClosed",
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

fn json_of<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// Subscribe a scripted `$fn` listener; the outcome is the unsubscribe.
fn listen<E: serde::Serialize>(
    h: &Harness,
    spec: Option<&Value>,
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> epdf_kernel::Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &E| match callback.call(vec![json_of(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
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
        h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<ShellCapability>()?;
        let id = arg(&args, 0).and_then(Value::as_str).unwrap_or_default();
        Some(match method {
            "isOpen" => Outcome::Returned(json!(cap.is_open(id))),
            "getSurface" => Outcome::Returned(json_of(&cap.get_surface(id))),
            "listOpenSurfaces" => Outcome::Returned(json_of(&cap.list_open_surfaces())),
            "open" => {
                cap.open(id, options(arg(&args, 1)));
                Outcome::undefined()
            }
            "close" => {
                cap.close(id);
                Outcome::undefined()
            }
            "toggle" => {
                cap.toggle(id, options(arg(&args, 1)));
                Outcome::undefined()
            }
            "updateSurfaceProps" => {
                let props: SurfaceProps = arg(&args, 1)
                    .map(scrub)
                    .and_then(|p| p.as_object().cloned())
                    .unwrap_or_default();
                {
                    cap.update_surface_props(id, props);
                    Outcome::undefined()
                }
            }
            "closeAll" => {
                cap.close_all();
                Outcome::undefined()
            }
            "isMenuOpen" => Outcome::Returned(json!(cap.is_menu_open(id))),
            "listOpenMenus" => Outcome::Returned(json!(cap.list_open_menus())),
            "openMenu" => {
                cap.open_menu(id);
                Outcome::undefined()
            }
            "closeMenu" => {
                cap.close_menu(id);
                Outcome::undefined()
            }
            "toggleMenu" => {
                cap.toggle_menu(id);
                Outcome::undefined()
            }
            "closeAllMenus" => {
                cap.close_all_menus();
                Outcome::undefined()
            }
            "getSnapshot" => Outcome::Returned(json_of(&cap.get_snapshot())),
            "applySnapshot" => match serde_json::from_value::<ShellSnapshot>(scrub(
                arg(&args, 0).unwrap_or(&Value::Null),
            )) {
                Ok(snapshot) => {
                    cap.apply_snapshot(snapshot);
                    Outcome::undefined()
                }
                Err(error) => Outcome::Threw(bad("snapshot", error)),
            },
            "onSurfaceOpened" => listen(h, arg(&args, 0), |f| cap.on_surface_opened(f)),
            "onSurfaceClosed" => listen(h, arg(&args, 0), |f| cap.on_surface_closed(f)),
            "onMenuOpened" => listen(h, arg(&args, 0), |f| cap.on_menu_opened(f)),
            "onMenuClosed" => listen(h, arg(&args, 0), |f| cap.on_menu_closed(f)),
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
