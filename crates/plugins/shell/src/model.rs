//! `model.ts`: the shell's state, which surfaces are open (with their
//! exclusivity tag, props and focus requests) and the stack of open menus.
//! Every function below is a pure transition; the controller applies them
//! with `ctx.state.update` ([`Controller::update_state`]).
//!
//! The state keeps the TypeScript's reference identity: `surfaces`, each
//! record and `openMenus` are `Rc`s, a transition that `return state`s hands
//! back the same `Rc`, and one that rebuilds `surfaces` leaves the records it
//! did not touch shared. The controller's events and memos compare them with
//! `Rc::ptr_eq`, as the TypeScript compares with `===`.
//!
//! [`Controller::update_state`]: epdf_kernel::Controller::update_state

use std::fmt;
use std::rc::Rc;

use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, Serializer};

use crate::contract::{OpenSurfaceOptions, ShellSnapshot, SurfaceProps};
use crate::record::JsRecord;

// ── member order ─────────────────────────────────────────────────────────────

/// A member of a surface record. The TypeScript builds records with object
/// literals and spreads, so the order a record prints its members in depends
/// on how it was made: `openSurface` writes `open, exclusive, props,
/// focusRequest`, a spread keeps the order it copies and appends a member it
/// adds (`setSurfaceProps` on a record restored without props puts `props`
/// last). [`Members`] is that order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Member {
    Open,
    Exclusive,
    Props,
    FocusRequest,
}

impl Member {
    fn named(name: &str) -> Option<Member> {
        Some(match name {
            "open" => Member::Open,
            "exclusive" => Member::Exclusive,
            "props" => Member::Props,
            "focusRequest" => Member::FocusRequest,
            _ => return None,
        })
    }
}

/// The own members of a record object, in property order. A member can be
/// own and `undefined` (`exclusive: options.exclusive` with no tag): it
/// prints nothing, but keeps its place for a later assignment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Members(Vec<Member>);

impl Members {
    /// The `openSurface` literal: `{ open, exclusive, props, focusRequest }`.
    fn literal() -> Members {
        Members(vec![Member::Open, Member::Exclusive, Member::Props, Member::FocusRequest])
    }

    /// `{ ...record, [member]: value }`: an own member keeps its place, a
    /// new one goes last.
    fn with(&self, member: Member) -> Members {
        let mut members = self.clone();
        if !members.0.contains(&member) {
            members.0.push(member);
        }
        members
    }

    /// `const { [member]: _, ...rest } = record`.
    fn without(&self, member: Member) -> Members {
        Members(self.0.iter().copied().filter(|m| *m != member).collect())
    }

    fn iter(&self) -> impl Iterator<Item = Member> + '_ {
        self.0.iter().copied()
    }
}

/// The members a record object was read with, as they arrived.
#[derive(Default)]
struct RawRecord {
    open: Option<bool>,
    exclusive: Option<String>,
    props: Option<SurfaceProps>,
    focus_request: Option<f64>,
    members: Members,
}

impl<'de> Deserialize<'de> for RawRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RecordVisitor;
        impl<'de> Visitor<'de> for RecordVisitor {
            type Value = RawRecord;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a surface record object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut access: M) -> Result<RawRecord, M::Error> {
                let mut raw = RawRecord::default();
                while let Some(key) = access.next_key::<String>()? {
                    let Some(member) = Member::named(&key) else {
                        // A member no surface record has; the typed record
                        // has nowhere to keep it.
                        access.next_value::<de::IgnoredAny>()?;
                        continue;
                    };
                    match member {
                        Member::Open => raw.open = Some(access.next_value()?),
                        Member::Exclusive => raw.exclusive = Some(access.next_value()?),
                        Member::Props => raw.props = Some(access.next_value()?),
                        Member::FocusRequest => raw.focus_request = Some(access.next_value()?),
                    }
                    raw.members = raw.members.with(member);
                }
                Ok(raw)
            }
        }
        deserializer.deserialize_map(RecordVisitor)
    }
}

/// Write `members` in order, each one that has a value.
fn serialize_members<S: SerializeMap>(
    map: &mut S,
    members: &Members,
    open: bool,
    exclusive: Option<&String>,
    props: Option<&SurfaceProps>,
    focus_request: Option<f64>,
) -> Result<(), S::Error> {
    for member in members.iter() {
        match member {
            Member::Open => map.serialize_entry("open", &open)?,
            Member::Exclusive => {
                if let Some(exclusive) = exclusive {
                    map.serialize_entry("exclusive", exclusive)?;
                }
            }
            Member::Props => {
                if let Some(props) = props {
                    map.serialize_entry("props", props)?;
                }
            }
            Member::FocusRequest => {
                if let Some(focus_request) = focus_request {
                    map.serialize_entry(
                        "focusRequest",
                        &epdf_kernel::js::json_number(focus_request),
                    )?;
                }
            }
        }
    }
    Ok(())
}

// ── records ──────────────────────────────────────────────────────────────────

/// `SurfaceRecord`: one surface's record; the surface id is its key.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceRecord {
    pub open: bool,
    /// The exclusivity tag the surface was opened with, if any.
    pub exclusive: Option<String>,
    /// Opaque props passed at open.
    pub props: Option<SurfaceProps>,
    /// Raised by every open that asks for focus; every way of closing sets
    /// it back to 0.
    pub focus_request: f64,
    members: Members,
}

impl SurfaceRecord {
    /// The record `openSurface` writes: `{ open, exclusive, props,
    /// focusRequest }`.
    pub fn new(
        open: bool,
        exclusive: Option<String>,
        props: Option<SurfaceProps>,
        focus_request: f64,
    ) -> SurfaceRecord {
        SurfaceRecord { open, exclusive, props, focus_request, members: Members::literal() }
    }

    /// The member order, for serializing a view that spreads the record.
    pub(crate) fn serialize_into<S: SerializeMap>(&self, map: &mut S) -> Result<(), S::Error> {
        serialize_members(
            map,
            &self.members,
            self.open,
            self.exclusive.as_ref(),
            self.props.as_ref(),
            Some(self.focus_request),
        )
    }
}

impl Serialize for SurfaceRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        self.serialize_into(&mut map)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for SurfaceRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawRecord::deserialize(deserializer)?;
        Ok(SurfaceRecord {
            open: raw.open.ok_or_else(|| de::Error::missing_field("open"))?,
            exclusive: raw.exclusive,
            props: raw.props,
            focus_request: raw
                .focus_request
                .ok_or_else(|| de::Error::missing_field("focusRequest"))?,
            members: raw.members,
        })
    }
}

/// One surface of a [`ShellSnapshot`]: `Omit<SurfaceState, 'id' |
/// 'focusRequest'>`.
#[derive(Clone, Debug, PartialEq)]
pub struct SnapshotSurface {
    pub open: bool,
    pub exclusive: Option<String>,
    pub props: Option<SurfaceProps>,
    /// Where the record had a `focusRequest`, if the object it was read from
    /// had one: `applySnapshot` writes 0 in that place. It never prints.
    members: Members,
}

impl SnapshotSurface {
    /// `{ open, exclusive, props }`.
    pub fn new(open: bool, exclusive: Option<String>, props: Option<SurfaceProps>) -> Self {
        SnapshotSurface {
            open,
            exclusive,
            props,
            members: Members(vec![Member::Open, Member::Exclusive, Member::Props]),
        }
    }
}

impl Serialize for SnapshotSurface {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        serialize_members(
            &mut map,
            &self.members,
            self.open,
            self.exclusive.as_ref(),
            self.props.as_ref(),
            None,
        )?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for SnapshotSurface {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawRecord::deserialize(deserializer)?;
        Ok(SnapshotSurface {
            open: raw.open.ok_or_else(|| de::Error::missing_field("open"))?,
            exclusive: raw.exclusive,
            props: raw.props,
            members: raw.members,
        })
    }
}

// ── state ────────────────────────────────────────────────────────────────────

pub type Surfaces = JsRecord<Rc<SurfaceRecord>>;

/// `ShellState`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellState {
    pub surfaces: Rc<Surfaces>,
    /// Open dropdown menus, in opening order (last = topmost).
    pub open_menus: Rc<Vec<String>>,
}

/// `initialShellState()`.
pub fn initial_shell_state() -> ShellState {
    ShellState { surfaces: Rc::new(JsRecord::new()), open_menus: Rc::new(Vec::new()) }
}

/// A surface closed: no longer open, and no request for focus left.
fn closed(surface: &SurfaceRecord) -> SurfaceRecord {
    SurfaceRecord {
        open: false,
        focus_request: 0.0,
        members: surface.members.with(Member::Open).with(Member::FocusRequest),
        ..surface.clone()
    }
}

/// Close every open surface that shares the exclusivity tag, except
/// `keep_id`.
fn close_exclusive(surfaces: &Surfaces, exclusive: &str, keep_id: &str) -> Surfaces {
    surfaces
        .iter()
        .map(|(id, surface)| {
            let closes =
                surface.open && surface.exclusive.as_deref() == Some(exclusive) && id != keep_id;
            (id.to_string(), if closes { Rc::new(closed(surface)) } else { surface.clone() })
        })
        .collect()
}

fn with_surfaces(state: &ShellState, surfaces: Surfaces) -> Rc<ShellState> {
    Rc::new(ShellState { surfaces: Rc::new(surfaces), open_menus: state.open_menus.clone() })
}

fn with_menus(state: &ShellState, open_menus: Vec<String>) -> Rc<ShellState> {
    Rc::new(ShellState { surfaces: state.surfaces.clone(), open_menus: Rc::new(open_menus) })
}

/// `openSurface(state, id, options = {})`.
pub fn open_surface(
    state: &Rc<ShellState>,
    id: &str,
    options: &OpenSurfaceOptions,
) -> Rc<ShellState> {
    // `options.exclusive ? ... : ...`: the empty tag is falsy.
    let mut surfaces = match options.exclusive.as_deref().filter(|tag| !tag.is_empty()) {
        Some(tag) => close_exclusive(&state.surfaces, tag, id),
        None => (*state.surfaces).clone(),
    };
    // A closed surface's request is 0, so only an open one carries a count
    // forward.
    let asked = state.surfaces.get(id).map_or(0.0, |surface| surface.focus_request);
    surfaces.insert(
        id,
        Rc::new(SurfaceRecord::new(
            true,
            options.exclusive.clone(),
            options.props.clone(),
            if options.focus { asked + 1.0 } else { asked },
        )),
    );
    with_surfaces(state, surfaces)
}

/// `closeSurface(state, id)`.
pub fn close_surface(state: &Rc<ShellState>, id: &str) -> Rc<ShellState> {
    let Some(surface) = state.surfaces.get(id).filter(|surface| surface.open) else {
        return state.clone();
    };
    let mut surfaces = (*state.surfaces).clone();
    surfaces.insert(id, Rc::new(closed(surface)));
    with_surfaces(state, surfaces)
}

/// `toggleSurface(state, id, options)`.
pub fn toggle_surface(
    state: &Rc<ShellState>,
    id: &str,
    options: &OpenSurfaceOptions,
) -> Rc<ShellState> {
    if state.surfaces.get(id).is_some_and(|surface| surface.open) {
        close_surface(state, id)
    } else {
        open_surface(state, id, options)
    }
}

/// `setSurfaceProps(state, id, props)`: `{ ...surface, props }`, which puts
/// `props` last on a record that had none. An id the record does not own
/// is ignored, `Object.prototype` names included (see the README).
pub fn set_surface_props(state: &Rc<ShellState>, id: &str, props: &SurfaceProps) -> Rc<ShellState> {
    let Some(surface) = state.surfaces.get(id) else {
        return state.clone();
    };
    let mut surfaces = (*state.surfaces).clone();
    surfaces.insert(
        id,
        Rc::new(SurfaceRecord {
            props: Some(props.clone()),
            members: surface.members.with(Member::Props),
            ..(**surface).clone()
        }),
    );
    with_surfaces(state, surfaces)
}

/// `closeAllSurfaces(state)`.
pub fn close_all_surfaces(state: &Rc<ShellState>) -> Rc<ShellState> {
    if !state.surfaces.values().any(|surface| surface.open) {
        return state.clone();
    }
    let surfaces = state
        .surfaces
        .iter()
        .map(|(id, surface)| {
            let next = if surface.open { Rc::new(closed(surface)) } else { surface.clone() };
            (id.to_string(), next)
        })
        .collect();
    with_surfaces(state, surfaces)
}

/// `openMenu(state, id)`.
pub fn open_menu(state: &Rc<ShellState>, id: &str) -> Rc<ShellState> {
    if state.open_menus.iter().any(|menu| menu == id) {
        return state.clone();
    }
    let mut open_menus = (*state.open_menus).clone();
    open_menus.push(id.to_string());
    with_menus(state, open_menus)
}

/// `closeMenu(state, id)`.
pub fn close_menu(state: &Rc<ShellState>, id: &str) -> Rc<ShellState> {
    if !state.open_menus.iter().any(|menu| menu == id) {
        return state.clone();
    }
    with_menus(state, state.open_menus.iter().filter(|menu| *menu != id).cloned().collect())
}

/// `toggleMenu(state, id)`.
pub fn toggle_menu(state: &Rc<ShellState>, id: &str) -> Rc<ShellState> {
    if state.open_menus.iter().any(|menu| menu == id) {
        close_menu(state, id)
    } else {
        open_menu(state, id)
    }
}

/// `closeAllMenus(state)`.
pub fn close_all_menus(state: &Rc<ShellState>) -> Rc<ShellState> {
    if state.open_menus.is_empty() { state.clone() } else { with_menus(state, Vec::new()) }
}

/// `snapshotOf(state)`: the state without the focus requests, which are
/// moments.
pub fn snapshot_of(state: &ShellState) -> ShellSnapshot {
    let surfaces = state
        .surfaces
        .iter()
        .map(|(id, surface)| {
            let surface = SnapshotSurface {
                open: surface.open,
                exclusive: surface.exclusive.clone(),
                props: surface.props.clone(),
                members: surface.members.without(Member::FocusRequest),
            };
            (id.to_string(), surface)
        })
        .collect();
    ShellSnapshot { surfaces, open_menus: state.open_menus.clone() }
}

/// `applySnapshot(_state, snapshot)`: every surface `{ ...surface,
/// focusRequest: 0 }`, and a copy of the menus.
pub fn apply_snapshot(_state: &Rc<ShellState>, snapshot: &ShellSnapshot) -> Rc<ShellState> {
    let surfaces = snapshot
        .surfaces
        .iter()
        .map(|(id, surface)| {
            let record = SurfaceRecord {
                open: surface.open,
                exclusive: surface.exclusive.clone(),
                props: surface.props.clone(),
                focus_request: 0.0,
                members: surface.members.with(Member::FocusRequest),
            };
            (id.to_string(), Rc::new(record))
        })
        .collect();
    Rc::new(ShellState {
        surfaces: Rc::new(surfaces),
        open_menus: Rc::new((*snapshot.open_menus).clone()),
    })
}
