//! The shell plugin through uniffi: one handle per document over its
//! surfaces and menus, one event enum. Every member is synchronous.
//!
//! A surface's `props` is an opaque JSON object by contract
//! (`SurfaceProps`), with no schema to mirror. It crosses as `props_json`,
//! the object as JSON text, and anything that is not an object is refused
//! with `invalid-input` before the plugin sees it.
//!
//! Migrated by hand to the 13.28 port (named transitions, `focusRequest`,
//! per-kind surface events); compiled and tested since Task 13.33.

use std::sync::Arc;

use epdf_ffi::{KernelBound, KernelPost, Subscription, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::store::Unsubscribe;
use serde_json::Value;

use crate::contract::{
    MenuClosedEvent, MenuOpenedEvent, OpenSurfaceOptions, SurfaceClosedEvent, SurfaceOpenedEvent,
    SurfaceProps, SurfaceState,
};
use crate::controller::ShellCapability;
use crate::model::SnapshotSurface;
use crate::record::JsRecord;

/// A surface's open flag, exclusivity tag, props and focus request. A name
/// the plugin finds on `Object.prototype` (`toString`) has no flag there;
/// it reads as closed here. Not `Eq`, because `focus_request` is an `f64`
/// (the JavaScript number).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ShellSurface {
    pub id: String,
    pub open: bool,
    pub exclusive: Option<String>,
    /// A JSON object, or `None` when the surface has no props.
    pub props_json: Option<String>,
    /// How many opens asked for focus since the surface opened; 0 once it
    /// closes. A rise is the moment to move the cursor into the surface.
    pub focus_request: f64,
}

/// One surface of a [`ShellSnapshot`]: no focus request, which is a moment
/// and not state to keep.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ShellSnapshotSurface {
    pub id: String,
    pub open: bool,
    pub exclusive: Option<String>,
    /// A JSON object, or `None` when the surface has no props.
    pub props_json: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct ShellOpenOptions {
    /// Opening closes every other open surface with the same tag.
    #[uniffi(default = None)]
    pub exclusive: Option<String>,
    /// A JSON object.
    #[uniffi(default = None)]
    pub props_json: Option<String>,
    /// Raise the surface's `focus_request`, also when it is open already.
    #[uniffi(default = false)]
    pub focus: bool,
}

/// Everything the shell holds for one document but the focus requests, for
/// persist and restore. Surfaces are in JavaScript property order
/// (integer-like ids first), closed ones included; menus are in opening
/// order, last is topmost.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ShellSnapshot {
    pub surfaces: Vec<ShellSnapshotSurface>,
    pub open_menus: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ShellSurfaceOpenedEvent {
    pub id: String,
    /// The props it was opened with.
    pub props_json: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ShellSurfaceClosedEvent {
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ShellMenuEvent {
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ShellEvent {
    SurfaceOpened { event: ShellSurfaceOpenedEvent },
    SurfaceClosed { event: ShellSurfaceClosedEvent },
    MenuOpened { event: ShellMenuEvent },
    MenuClosed { event: ShellMenuEvent },
}

#[uniffi::export(with_foreign)]
pub trait ShellEventListener: Send + Sync {
    fn on_event(&self, event: ShellEvent);
    /// The document closed: no event follows. Called once, and at once for
    /// a subscription made after the close.
    fn on_end(&self);
}

fn props_to_json(props: &SurfaceProps) -> String {
    Value::Object(props.clone()).to_string()
}

fn props_from_json(json: &str) -> Result<SurfaceProps, Error> {
    match serde_json::from_str::<Value>(json) {
        Ok(Value::Object(props)) => Ok(props),
        Ok(other) => Err(invalid_props(format!("props must be a JSON object, not {other}"))),
        Err(e) => Err(invalid_props(format!("props are not JSON: {e}"))),
    }
}

fn invalid_props(message: String) -> Error {
    plugin_error(code::INVALID_INPUT, "shell", message, None, None)
}

fn optional_props(json: Option<&str>) -> Result<Option<SurfaceProps>, Error> {
    json.map(props_from_json).transpose()
}

impl From<&SurfaceState> for ShellSurface {
    fn from(state: &SurfaceState) -> Self {
        ShellSurface {
            id: state.id.clone(),
            open: state.open(),
            exclusive: state.exclusive().map(str::to_string),
            props_json: state.props().map(props_to_json),
            focus_request: state.focus_request(),
        }
    }
}

impl TryFrom<ShellOpenOptions> for OpenSurfaceOptions {
    type Error = Error;

    fn try_from(options: ShellOpenOptions) -> Result<Self, Error> {
        Ok(OpenSurfaceOptions {
            exclusive: options.exclusive,
            props: optional_props(options.props_json.as_deref())?,
            focus: options.focus,
        })
    }
}

impl From<&crate::ShellSnapshot> for ShellSnapshot {
    fn from(snapshot: &crate::ShellSnapshot) -> Self {
        ShellSnapshot {
            surfaces: snapshot
                .surfaces
                .iter()
                .map(|(id, surface)| ShellSnapshotSurface {
                    id: id.to_string(),
                    open: surface.open,
                    exclusive: surface.exclusive.clone(),
                    props_json: surface.props.as_ref().map(props_to_json),
                })
                .collect(),
            open_menus: (*snapshot.open_menus).clone(),
        }
    }
}

impl TryFrom<ShellSnapshot> for crate::ShellSnapshot {
    type Error = Error;

    fn try_from(snapshot: ShellSnapshot) -> Result<Self, Error> {
        let surfaces = snapshot
            .surfaces
            .into_iter()
            .map(|s| {
                let props = optional_props(s.props_json.as_deref())?;
                Ok((s.id, SnapshotSurface::new(s.open, s.exclusive, props)))
            })
            .collect::<Result<JsRecord<SnapshotSurface>, Error>>()?;
        Ok(crate::ShellSnapshot { surfaces, open_menus: std::rc::Rc::new(snapshot.open_menus) })
    }
}

impl From<&SurfaceOpenedEvent> for ShellSurfaceOpenedEvent {
    fn from(event: &SurfaceOpenedEvent) -> Self {
        ShellSurfaceOpenedEvent {
            id: event.id.clone(),
            props_json: event.props.as_ref().map(props_to_json),
        }
    }
}

impl From<&SurfaceClosedEvent> for ShellSurfaceClosedEvent {
    fn from(event: &SurfaceClosedEvent) -> Self {
        ShellSurfaceClosedEvent { id: event.id.clone() }
    }
}

impl From<&MenuOpenedEvent> for ShellMenuEvent {
    fn from(event: &MenuOpenedEvent) -> Self {
        ShellMenuEvent { id: event.id.clone() }
    }
}

impl From<&MenuClosedEvent> for ShellMenuEvent {
    fn from(event: &MenuClosedEvent) -> Self {
        ShellMenuEvent { id: event.id.clone() }
    }
}

/// The capability, if its document is still open.
fn open(cap: &ShellCapability) -> Result<&ShellCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "shell",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

#[derive(uniffi::Object)]
pub struct ShellHandle {
    cap: KernelBound<ShellCapability>,
    post: KernelPost,
}

impl ShellHandle {
    /// On the kernel thread.
    pub fn new(cap: ShellCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(ShellHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl ShellHandle {
    pub async fn is_open(&self, id: String) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.is_open(&id))).await
    }

    /// `None` for a surface never opened.
    pub async fn surface(&self, id: String) -> Result<Option<ShellSurface>, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.get_surface(&id).map(|s| (&*s).into()))).await
    }

    /// Open surfaces, in JavaScript property order (`listOpenSurfaces`).
    pub async fn open_surfaces(&self) -> Result<Vec<ShellSurface>, ViewerError> {
        self.cap
            .call(|c| Ok(open(c)?.list_open_surfaces().iter().map(|s| (&**s).into()).collect()))
            .await
    }

    /// `open`. A tagged open closes every other open surface with that tag.
    pub async fn open_surface(
        &self,
        id: String,
        options: Option<ShellOpenOptions>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let options = options.map(OpenSurfaceOptions::try_from).transpose()?;
                open(c)?.open(&id, options);
                Ok(())
            })
            .await
    }

    /// `close`.
    pub async fn close_surface(&self, id: String) -> Result<(), ViewerError> {
        self.cap.call(move |c| open(c).map(|c| c.close(&id))).await
    }

    /// `toggle`.
    pub async fn toggle_surface(
        &self,
        id: String,
        options: Option<ShellOpenOptions>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let options = options.map(OpenSurfaceOptions::try_from).transpose()?;
                open(c)?.toggle(&id, options);
                Ok(())
            })
            .await
    }

    /// Change a surface's props without reopening it. An unknown id is
    /// ignored.
    pub async fn update_surface_props(
        &self,
        id: String,
        props_json: String,
    ) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let props = props_from_json(&props_json)?;
                open(c)?.update_surface_props(&id, props);
                Ok(())
            })
            .await
    }

    /// `closeAll`.
    pub async fn close_all_surfaces(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| open(c).map(|c| c.close_all())).await
    }

    pub async fn is_menu_open(&self, id: String) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.is_menu_open(&id))).await
    }

    /// In opening order; the last is topmost (`listOpenMenus`).
    pub async fn open_menus(&self) -> Result<Vec<String>, ViewerError> {
        self.cap.call(|c| Ok((*open(c)?.list_open_menus()).clone())).await
    }

    pub async fn open_menu(&self, id: String) -> Result<(), ViewerError> {
        self.cap.call(move |c| open(c).map(|c| c.open_menu(&id))).await
    }

    pub async fn close_menu(&self, id: String) -> Result<(), ViewerError> {
        self.cap.call(move |c| open(c).map(|c| c.close_menu(&id))).await
    }

    pub async fn toggle_menu(&self, id: String) -> Result<(), ViewerError> {
        self.cap.call(move |c| open(c).map(|c| c.toggle_menu(&id))).await
    }

    pub async fn close_all_menus(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| open(c).map(|c| c.close_all_menus())).await
    }

    /// `getSnapshot`.
    pub async fn snapshot(&self) -> Result<ShellSnapshot, ViewerError> {
        self.cap.call(|c| Ok((&*open(c)?.get_snapshot()).into())).await
    }

    /// Replace the state with `snapshot`. A surface whose props aren't a
    /// JSON object refuses the whole snapshot.
    pub async fn apply_snapshot(&self, snapshot: ShellSnapshot) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                let snapshot = crate::ShellSnapshot::try_from(snapshot)?;
                open(c)?.apply_snapshot(&snapshot);
                Ok(())
            })
            .await
    }

    /// Registered on the kernel thread after this returns; a cancel before
    /// then still takes effect. `on_end` comes when the document closes (at
    /// once if it already has).
    pub fn subscribe(&self, listener: Arc<dyn ShellEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let (opened, closed, menu_opened, menu_closed) =
                        (listener.clone(), listener.clone(), listener.clone(), listener);
                    let offs = [
                        c.on_surface_opened(move |e| {
                            opened.on_event(ShellEvent::SurfaceOpened { event: e.into() });
                            Ok(())
                        }),
                        c.on_surface_closed(move |e| {
                            closed.on_event(ShellEvent::SurfaceClosed { event: e.into() });
                            Ok(())
                        }),
                        c.on_menu_opened(move |e| {
                            menu_opened.on_event(ShellEvent::MenuOpened { event: e.into() });
                            Ok(())
                        }),
                        c.on_menu_closed(move |e| {
                            menu_closed.on_event(ShellEvent::MenuClosed { event: e.into() });
                            Ok(())
                        }),
                    ];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
