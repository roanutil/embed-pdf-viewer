//! `flags.ts`: the one interpretation of the `/F` flags (ISO 32000-2 Table
//! 167). Rendering, hit-testing, chrome and the reducer all ask here.

use serde_json::Value;

use crate::engine::facts::FLAG_NAMES;
use crate::json::{self, Object};
use crate::record::kind_of;
use crate::types::{Authority, ModelAnnotation};

/// The flag names, in the primitives' order (`FLAG_KEYS`).
pub const FLAG_KEYS: [&str; 10] = FLAG_NAMES;

/// `NO_ANNOTATION_FLAGS`.
pub fn no_annotation_flags() -> Object {
    FLAG_KEYS.iter().map(|k| ((*k).to_string(), Value::Bool(false))).collect()
}

/// `DRAWN_FLAGS`: `print` set, the rest off.
pub fn drawn_flags() -> Object {
    let mut flags = no_annotation_flags();
    flags.insert("print".into(), Value::Bool(true));
    flags
}

/// `flagsEqual(left, right)`: every flag `===`.
pub fn flags_equal(left: &Object, right: &Object) -> bool {
    FLAG_KEYS.iter().all(|k| left.get(*k) == right.get(*k))
}

/// `mergeFlags(base, patch)`.
pub fn merge_flags(base: &Object, patch: &Object) -> Object {
    json::spread(base, patch)
}

fn flag(annotation: &Object, name: &str) -> bool {
    json::truthy(annotation.get(name))
}

/// Does `invisible` leave this annotation unpainted? Only on a type that isn't standard.
fn hidden_by_invisible_flag(annotation: &Object) -> bool {
    flag(annotation, "invisible")
        && json::get_str(annotation, "subtype") == Some("unsupported")
        && json::get_num(annotation, "rawSubtypeCode") == Some(0.0)
        && json::get_str(annotation, "rawSubtypeName") != Some("Projection")
}

/// `viewable(annotation, engaged)`: on-screen visibility.
pub fn viewable(annotation: &Object, engaged: bool) -> bool {
    !flag(annotation, "hidden")
        && !hidden_by_invisible_flag(annotation)
        && (!flag(annotation, "noView") || (flag(annotation, "toggleNoView") && engaged))
}

/// `interactive(flags)`: any pointer interaction.
pub fn interactive(flags: &Object) -> bool {
    !flag(flags, "hidden") && !flag(flags, "noView") && !flag(flags, "readOnly")
}

/// The part of a record the predicates read (`FlagBearer`).
pub trait FlagBearer {
    fn bearer_annotation(&self) -> &Object;
    fn bearer_authority(&self) -> Option<&Authority>;
}

impl FlagBearer for ModelAnnotation {
    fn bearer_annotation(&self) -> &Object {
        &self.annotation
    }
    fn bearer_authority(&self) -> Option<&Authority> {
        self.authority.as_ref()
    }
}

fn may_update(record: &dyn FlagBearer) -> bool {
    record.bearer_authority().is_none_or(|a| a.update)
}

fn may_delete(record: &dyn FlagBearer) -> bool {
    record.bearer_authority().is_none_or(|a| a.delete)
}

/// `annotInteractive(record)`: widgets ignore `readOnly`.
pub fn annot_interactive(record: &dyn FlagBearer) -> bool {
    let annotation = record.bearer_annotation();
    if kind_of(annotation).caps.ignores_read_only {
        !flag(annotation, "hidden") && !flag(annotation, "noView")
    } else {
        interactive(annotation)
    }
}

/// `annotTransformable(record)`.
pub fn annot_transformable(record: &dyn FlagBearer) -> bool {
    annot_interactive(record) && !flag(record.bearer_annotation(), "locked") && may_update(record)
}

/// `annotDeletable(record)`.
pub fn annot_deletable(record: &dyn FlagBearer) -> bool {
    annot_interactive(record) && !flag(record.bearer_annotation(), "locked") && may_delete(record)
}

/// `annotContentsEditable(record)`.
pub fn annot_contents_editable(record: &dyn FlagBearer) -> bool {
    annot_interactive(record)
        && !flag(record.bearer_annotation(), "lockedContents")
        && may_update(record)
}

/// `mayWrite(record, field)`: may this session change the engine field now?
pub fn may_write(record: &dyn FlagBearer, field: &str) -> bool {
    if FLAG_KEYS.contains(&field) {
        return may_update(record);
    }
    if field == "contents" || field == "richText" {
        annot_contents_editable(record)
    } else {
        annot_transformable(record)
    }
}
