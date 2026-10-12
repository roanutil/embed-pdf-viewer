//! `read/selection-props.ts`: the selection's editable properties and `/F`
//! flags, ready for a sidebar. While the editor holds a text range in the
//! sole selected free text, the range keys report the runs it covers.

use std::rc::Rc;

use epdf_annotation_core::richtext::RichTextRange;
use epdf_annotation_core::{Annot, FLAG_KEYS, Model, PropKey, link_of, read_prop, shared_props};
use epdf_kernel::Error;
use serde_json::{Map, Value};

use crate::contract::*;
use crate::controller::AnnotationCapability;
use crate::rich_text::{RANGE_KEYS, range_props, rich_doc_of};

impl AnnotationCapability {
    /// `activeTextRange(m)`: the editor's range inside the annotation being
    /// edited, or `None` (no editor selection, a bare caret, or a selection
    /// left behind by a previous edit).
    pub(crate) fn active_text_range(&self, m: &Model) -> Option<Rc<TextSelection>> {
        let ts = self.state().text_selection.clone()?;
        (m.editing.as_deref() == Some(ts.id.as_str())
            && ts.end > ts.start
            && m.by_id.contains_key(&ts.id))
        .then_some(ts)
    }

    /// `getSelectionProps()`. Fails where the TypeScript throws: a text
    /// range over runs whose resolved style has no color.
    pub fn get_selection_props(&self) -> Result<SelectionProps, Error> {
        let m = self.model();
        let range = self.active_text_range(&m);
        let members: Vec<&Annot> = m.selected.iter().filter_map(|id| m.by_id.get(id)).collect();
        let subtypes: Vec<String> = members.iter().map(|a| a.subtype.clone()).collect();
        let specs = shared_props(&subtypes);
        let value_of = |a: &Annot, key: PropKey| -> Option<Value> {
            if key == PropKey::Link && a.subtype != "link" {
                return Some(
                    link_of(&m, &a.id)
                        .map_or(Value::Null, |t| serde_json::to_value(t).unwrap_or_default()),
                );
            }
            read_prop(a, key).map(|v| serde_json::to_value(v).unwrap_or_default())
        };
        let mut values = Map::new();
        let mut mixed = Vec::new();
        for spec in &specs {
            let first = value_of(members[0], spec.key);
            if let Some(v) = &first {
                values.insert(spec.key.as_str().into(), v.clone());
            }
            if members.iter().any(|a| value_of(a, spec.key) != first) {
                mixed.push(spec.key);
            }
        }
        if let Some(range) = range
            && members.len() == 1
            && members[0].id == range.id
        {
            let fonts = self.fonts();
            let (rv, rmixed) = range_props(
                &rich_doc_of(members[0], Some(&fonts)),
                RichTextRange { start: range.start, end: range.end },
                Some(&fonts),
            )?;
            for spec in &specs {
                if !RANGE_KEYS.contains(&spec.key) {
                    continue;
                }
                // `values[key] = rp.values[key]`: an undefined value leaves
                // the key out.
                match rv.get(spec.key.as_str()) {
                    Some(v) => values.insert(spec.key.as_str().into(), v.clone()),
                    None => values.remove(spec.key.as_str()),
                };
                mixed.retain(|k| *k != spec.key);
                if rmixed.contains(&spec.key) {
                    mixed.push(spec.key);
                }
            }
        }
        Ok(SelectionProps { specs, values, mixed })
    }

    /// `getSelectionFlags()`: `None` with nothing selected.
    pub fn get_selection_flags(&self) -> Option<SelectionFlags> {
        let m = self.model();
        let members: Vec<&Annot> = m.selected.iter().filter_map(|id| m.by_id.get(id)).collect();
        if members.is_empty() {
            return None;
        }
        let flags: Vec<Value> =
            members.iter().map(|a| serde_json::to_value(a.flags).unwrap_or_default()).collect();
        let mut out = Map::new();
        for key in FLAG_KEYS {
            let first = flags[0].get(key).cloned().unwrap_or(Value::Null);
            let agree = flags.iter().all(|f| f.get(key) == Some(&first));
            out.insert(key.to_string(), if agree { first } else { Value::Null });
        }
        Some(out)
    }
}
