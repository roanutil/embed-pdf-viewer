//! `group.ts`: group membership. A group is a primary and its `/RT /Group`
//! subordinates; attached link children never count as members.

use crate::editable::is_transformable_by_hand;
use crate::js_object;
use crate::kinds::KindCaps;
use crate::plane::is_attached_link;
use crate::record::{group_of, kind_of};
use crate::types::{Id, Model, ModelAnnotation};

js_object! {
    /// The transform capabilities of a multi-target selection (`GroupCaps`).
    pub struct GroupCaps {
        "movable" => pub movable: bool,
        "resizable" => pub resizable: bool,
        "rotatable" => pub rotatable: bool,
    }
}

fn visual_member(record: Option<&ModelAnnotation>) -> bool {
    record.is_some_and(|r| !is_attached_link(&r.annotation))
}

/// `groupCaps(model, ids)`.
pub fn group_caps(model: &Model, ids: &[Id]) -> GroupCaps {
    let members: Vec<&ModelAnnotation> = ids.iter().filter_map(|id| model.record(id)).collect();
    if members.is_empty() {
        return GroupCaps {
            movable: false,
            resizable: false,
            rotatable: false,
            order: Default::default(),
        };
    }
    let ok = |pick: fn(&KindCaps) -> bool| {
        members.iter().all(|r| {
            is_transformable_by_hand(&model.editable, r) && pick(&kind_of(&r.annotation).caps)
        })
    };
    GroupCaps {
        movable: ok(|c| c.group_movable),
        resizable: ok(|c| c.group_resizable),
        rotatable: ok(|c| c.group_rotatable),
        order: Default::default(),
    }
}

/// `groupKeyOf(model, id)`.
pub fn group_key_of(model: &Model, id: &str) -> Option<Id> {
    let record = model.record(id)?;
    if let Some(group) = group_of(&record.annotation) {
        return if is_attached_link(&record.annotation) { None } else { Some(group) };
    }
    for other in model.order.iter() {
        let sub = model.record(other);
        if sub.is_some_and(|s| group_of(&s.annotation).as_deref() == Some(id)) && visual_member(sub)
        {
            return Some(id.to_string());
        }
    }
    None
}

/// `groupMembers(model, id)`: primary first, or just `[id]`.
pub fn group_members(model: &Model, id: &str) -> Vec<Id> {
    let Some(key) = group_key_of(model, id) else { return vec![id.to_string()] };
    let mut members = Vec::new();
    if model.record(&key).is_some() {
        members.push(key.clone());
    }
    for other in model.order.iter() {
        let sub = model.record(other);
        if *other != key
            && sub.is_some_and(|s| group_of(&s.annotation).as_deref() == Some(key.as_str()))
            && visual_member(sub)
        {
            members.push(other.clone());
        }
    }
    members
}

/// `expandGroups(model, ids)`: every id's whole group.
pub fn expand_groups(model: &Model, ids: &[Id]) -> Vec<Id> {
    let mut out: Vec<Id> = Vec::new();
    for id in ids {
        for member in group_members(model, id) {
            if !out.contains(&member) {
                out.push(member);
            }
        }
    }
    out
}
