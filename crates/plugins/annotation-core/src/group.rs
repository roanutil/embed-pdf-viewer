//! `group.ts`: group membership. A subordinate carries `group = <primary's
//! id>`; the primary is the annotation whose id is the key. Attached link
//! children ride the same wire relationship but never count as members.

use serde::{Deserialize, Serialize};

use crate::flags::annot_transformable;
use crate::kinds::{KindCaps, caps_for};
use crate::plane::is_attached_link;
use crate::types::{Annot, Id, Model};

/// A subordinate that counts toward a visual group.
fn visual_member(a: Option<&Annot>) -> bool {
    a.is_some_and(|a| !is_attached_link(a))
}

/// The transform capabilities of a multi-target selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupCaps {
    pub movable: bool,
    pub resizable: bool,
    pub rotatable: bool,
}

pub fn group_caps(m: &Model, ids: &[Id]) -> GroupCaps {
    let members: Vec<&Annot> = ids.iter().filter_map(|id| m.by_id.get(id)).collect();
    if members.is_empty() {
        return GroupCaps { movable: false, resizable: false, rotatable: false };
    }
    let ok = |pick: fn(&KindCaps) -> bool| {
        members.iter().all(|a| annot_transformable(*a) && pick(&caps_for(&a.subtype)))
    };
    GroupCaps {
        movable: ok(|c| c.group_movable),
        resizable: ok(|c| c.group_resizable),
        rotatable: ok(|c| c.group_rotatable),
    }
}

/// The key of the group `id` belongs to, or `None` when it is ungrouped.
pub fn group_key_of(m: &Model, id: &str) -> Option<Id> {
    let a = m.by_id.get(id)?;
    if let Some(group) = a.group.as_ref().filter(|g| !g.is_empty()) {
        return if is_attached_link(a) { None } else { Some(group.clone()) };
    }
    for other in &m.order {
        let sub = m.by_id.get(other);
        if sub.is_some_and(|s| s.group.as_deref() == Some(id)) && visual_member(sub) {
            return Some(id.to_string());
        }
    }
    None
}

/// Every member of the group containing `id` (primary first), or `[id]`.
pub fn group_members(m: &Model, id: &str) -> Vec<Id> {
    let Some(key) = group_key_of(m, id) else {
        return vec![id.to_string()];
    };
    let mut members = Vec::new();
    if m.by_id.contains_key(&key) {
        members.push(key.clone());
    }
    for other in &m.order {
        let sub = m.by_id.get(other);
        if *other != key
            && sub.is_some_and(|s| s.group.as_deref() == Some(&key))
            && visual_member(sub)
        {
            members.push(other.clone());
        }
    }
    members
}

/// Union of every id's full group, first-seen order.
pub fn expand_groups(m: &Model, ids: &[Id]) -> Vec<Id> {
    let mut out: Vec<Id> = Vec::new();
    for id in ids {
        for member in group_members(m, id) {
            if !out.contains(&member) {
                out.push(member);
            }
        }
    }
    out
}
