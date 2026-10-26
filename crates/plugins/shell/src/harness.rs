//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, and the function
//! table the transition suite calls (`model.ts`'s exports).

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::{AnyPlugin, AnyToken, Error, Unsubscribe, is_undefined};
use plugin_conformance::{Answer, Callback, FunctionTable, Harness, HarnessPlugin, Outcome, scrub};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::contract::{OpenSurfaceOptions, ShellSnapshot, SurfaceProps};
use crate::model::{self, ShellState};
use crate::{SHELL, ShellCapability, initial_shell_state, shell_plugin};

pub struct Shell;

/// The TypeScript object literal's member order (`controller.ts`).
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

fn bad(what: &str, e: impl std::fmt::Display) -> Error {
    Error::new(format!("[shell harness] {what}: {e}"))
}

fn decode<T: DeserializeOwned>(args: &[Value], i: usize, what: &str) -> Result<T, Error> {
    serde_json::from_value(scrub(arg(args, i).unwrap_or(&Value::Null))).map_err(|e| bad(what, e))
}

/// JavaScript truthiness, for `options.focus ? ... : ...`.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => !is_undefined(value),
    }
}

fn options(value: Option<&Value>) -> Option<OpenSurfaceOptions> {
    let value = scrub(value?);
    Some(OpenSurfaceOptions {
        exclusive: value.get("exclusive").and_then(Value::as_str).map(str::to_string),
        props: value.get("props").and_then(Value::as_object).cloned(),
        focus: value.get("focus").is_some_and(truthy),
    })
}

fn props(value: Option<&Value>) -> SurfaceProps {
    value.map(scrub).and_then(|p| p.as_object().cloned()).unwrap_or_default()
}

fn json_of<T: Serialize>(value: &T) -> Value {
    epdf_kernel::js::to_value(value)
}

/// A scripted listener: the event as its JSON, a scripted throw reported.
fn listener<T: Serialize>(callback: Callback) -> impl Fn(&T) -> Result<(), Error> {
    move |event| match callback.call(vec![json_of(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }
}

/// The unsubscribe a listener verb returns, as a callable ref.
fn unsubscribe(off: Unsubscribe, h: &Harness) -> Outcome {
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
        let done = Outcome::undefined;
        Some(match method {
            "isOpen" => Outcome::Returned(json!(cap.is_open(id))),
            "getSurface" => Outcome::Returned(json_of(&cap.get_surface(id))),
            "listOpenSurfaces" => Outcome::Returned(json_of(&cap.list_open_surfaces())),
            "open" => {
                cap.open(id, options(arg(&args, 1)));
                done()
            }
            "close" => {
                cap.close(id);
                done()
            }
            "toggle" => {
                cap.toggle(id, options(arg(&args, 1)));
                done()
            }
            "updateSurfaceProps" => {
                cap.update_surface_props(id, props(arg(&args, 1)));
                done()
            }
            "closeAll" => {
                cap.close_all();
                done()
            }
            "isMenuOpen" => Outcome::Returned(json!(cap.is_menu_open(id))),
            "listOpenMenus" => Outcome::Returned(json_of(&cap.list_open_menus())),
            "openMenu" => {
                cap.open_menu(id);
                done()
            }
            "closeMenu" => {
                cap.close_menu(id);
                done()
            }
            "toggleMenu" => {
                cap.toggle_menu(id);
                done()
            }
            "closeAllMenus" => {
                cap.close_all_menus();
                done()
            }
            "getSnapshot" => Outcome::Returned(json_of(&cap.get_snapshot())),
            "applySnapshot" => match decode::<ShellSnapshot>(&args, 0, "snapshot") {
                Ok(snapshot) => {
                    cap.apply_snapshot(&snapshot);
                    done()
                }
                Err(error) => Outcome::Threw(error),
            },
            "onSurfaceOpened" => {
                let callback = h.callback(arg(&args, 0).unwrap_or(&Value::Null));
                unsubscribe(cap.on_surface_opened(listener(callback)), h)
            }
            "onSurfaceClosed" => {
                let callback = h.callback(arg(&args, 0).unwrap_or(&Value::Null));
                unsubscribe(cap.on_surface_closed(listener(callback)), h)
            }
            "onMenuOpened" => {
                let callback = h.callback(arg(&args, 0).unwrap_or(&Value::Null));
                unsubscribe(cap.on_menu_opened(listener(callback)), h)
            }
            "onMenuClosed" => {
                let callback = h.callback(arg(&args, 0).unwrap_or(&Value::Null));
                unsubscribe(cap.on_menu_closed(listener(callback)), h)
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

// ── the transition table ─────────────────────────────────────────────────────

fn state(args: &[Value]) -> Result<Rc<ShellState>, Error> {
    decode::<ShellState>(args, 0, "state").map(Rc::new)
}

fn id(args: &[Value]) -> &str {
    arg(args, 1).and_then(Value::as_str).unwrap_or_default()
}

fn shown(state: &ShellState) -> Result<Value, Error> {
    Ok(json_of(state))
}

fn f_initial(_: &[Value]) -> Result<Value, Error> {
    shown(&initial_shell_state())
}

fn f_open_surface(args: &[Value]) -> Result<Value, Error> {
    let options = options(arg(args, 2)).unwrap_or_default();
    shown(&model::open_surface(&state(args)?, id(args), &options))
}

fn f_close_surface(args: &[Value]) -> Result<Value, Error> {
    shown(&model::close_surface(&state(args)?, id(args)))
}

fn f_toggle_surface(args: &[Value]) -> Result<Value, Error> {
    let options = options(arg(args, 2)).unwrap_or_default();
    shown(&model::toggle_surface(&state(args)?, id(args), &options))
}

fn f_set_surface_props(args: &[Value]) -> Result<Value, Error> {
    shown(&model::set_surface_props(&state(args)?, id(args), &props(arg(args, 2))))
}

fn f_close_all_surfaces(args: &[Value]) -> Result<Value, Error> {
    shown(&model::close_all_surfaces(&state(args)?))
}

fn f_open_menu(args: &[Value]) -> Result<Value, Error> {
    shown(&model::open_menu(&state(args)?, id(args)))
}

fn f_close_menu(args: &[Value]) -> Result<Value, Error> {
    shown(&model::close_menu(&state(args)?, id(args)))
}

fn f_toggle_menu(args: &[Value]) -> Result<Value, Error> {
    shown(&model::toggle_menu(&state(args)?, id(args)))
}

fn f_close_all_menus(args: &[Value]) -> Result<Value, Error> {
    shown(&model::close_all_menus(&state(args)?))
}

fn f_snapshot_of(args: &[Value]) -> Result<Value, Error> {
    Ok(json_of(&model::snapshot_of(&*state(args)?)))
}

fn f_apply_snapshot(args: &[Value]) -> Result<Value, Error> {
    let snapshot: ShellSnapshot = decode(args, 1, "snapshot")?;
    shown(&model::apply_snapshot(&state(args)?, &snapshot))
}

pub fn functions() -> FunctionTable {
    vec![
        ("initialShellState", f_initial),
        ("openSurface", f_open_surface),
        ("closeSurface", f_close_surface),
        ("toggleSurface", f_toggle_surface),
        ("setSurfaceProps", f_set_surface_props),
        ("closeAllSurfaces", f_close_all_surfaces),
        ("openMenu", f_open_menu),
        ("closeMenu", f_close_menu),
        ("toggleMenu", f_toggle_menu),
        ("closeAllMenus", f_close_all_menus),
        ("snapshotOf", f_snapshot_of),
        ("applySnapshot", f_apply_snapshot),
    ]
}
