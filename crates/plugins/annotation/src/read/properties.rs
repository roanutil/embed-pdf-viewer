//! `read/properties.ts`: the selection's style-panel properties, memoized by
//! model identity. Only the members people may change by hand count.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_annotation_core::editable::is_editable_by_hand;
use epdf_annotation_core::links::link_of;
use epdf_annotation_core::props::shared_properties;
use epdf_annotation_core::record::{kind_of, text_of};
use epdf_annotation_core::richtext::RichTextRange;
use epdf_annotation_core::types::{Model, ModelAnnotation, TextStyle};
use serde_json::{Value, json};

use crate::controller::Core;
use crate::js::Object;
use crate::read::property_values::{PropertySources, property_value, property_values};
use crate::rich_text::{RANGE_KEYS, TextSelection, range_props, rich_doc_of};

type PropertiesCache = Option<(Rc<Model>, Option<Rc<TextSelection>>, Value)>;

#[derive(Default)]
pub struct PropertiesState {
    cache: RefCell<PropertiesCache>,
}

/// One member's sources, owned.
struct Sources {
    data: Object,
    text: Option<TextStyle>,
    link: Value,
}

impl Sources {
    fn view(&self) -> PropertySources<'_> {
        PropertySources {
            data: &self.data,
            text: self.text.as_ref(),
            link: Some(self.link.clone()),
        }
    }
}

fn opt_ptr<T>(a: &Option<Rc<T>>, b: &Option<Rc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

impl Core {
    /// `activeTextRange(model)`: the editor's text range inside the
    /// annotation being edited, or none.
    pub(crate) fn active_text_range(&self, model: &Model) -> Option<Rc<TextSelection>> {
        let ts = self.state().text_selection.clone()?;
        (model.editing.as_deref() == Some(ts.id.as_str())
            && ts.end > ts.start
            && model.by_id.contains_key(&ts.id))
        .then_some(ts)
    }

    fn sources_of(&self, model: &Model, record: &ModelAnnotation) -> Sources {
        let annotation = &record.annotation;
        let link = if annotation.get("subtype").and_then(Value::as_str) == Some("link") {
            annotation
                .get("target")
                .cloned()
                .filter(|t| !epdf_kernel::is_undefined(t))
                .unwrap_or(Value::Null)
        } else {
            link_of(model, &record.id)
        };
        Sources { data: (**annotation).clone(), text: text_of(annotation), link }
    }

    /// `selectionPropertiesOf()`: the same object while nothing changes.
    pub(crate) fn selection_properties(&self) -> Value {
        let model = self.model();
        let range = self.active_text_range(&model);
        if let Some((m, r, v)) = &*self.properties.cache.borrow()
            && Rc::ptr_eq(m, &model)
            && opt_ptr(r, &range)
        {
            return v.clone();
        }
        let members: Vec<&Rc<ModelAnnotation>> = model
            .selected
            .iter()
            .filter_map(|id| model.by_id.get(id))
            .filter(|r| is_editable_by_hand(&model.editable, &r.annotation))
            .collect();
        let kinds: Vec<_> = members.iter().map(|r| kind_of(&r.annotation)).collect();
        let properties = shared_properties(&kinds);
        let sources: Vec<Sources> = members.iter().map(|r| self.sources_of(&model, r)).collect();
        let mut values = match sources.first() {
            Some(first) => property_values(&properties, &first.view()),
            None => json!({}),
        };
        let mut mixed: Vec<String> = Vec::new();
        for spec in &properties {
            let Some(first) = sources.first() else { break };
            let first_text = crate::js::stringify(property_value(spec, &first.view()).as_ref());
            if sources.iter().any(|s| {
                crate::js::stringify(property_value(spec, &s.view()).as_ref()) != first_text
            }) {
                mixed.push(spec.get("key").and_then(Value::as_str).unwrap_or("").to_string());
            }
        }
        if let Some(range) = &range
            && members.len() == 1
            && members[0].id == range.id
        {
            let doc = rich_doc_of(&members[0].annotation, self.fonts());
            if let Ok((range_values, range_mixed)) = range_props(
                &doc,
                RichTextRange { start: range.start, end: range.end },
                self.fonts(),
            ) {
                for spec in &properties {
                    let key = spec.get("key").and_then(Value::as_str).unwrap_or("");
                    if !RANGE_KEYS.contains(&key) {
                        continue;
                    }
                    values[key] =
                        range_values.get(key).cloned().unwrap_or_else(epdf_kernel::undefined);
                    mixed.retain(|k| k != key);
                    if range_mixed.iter().any(|k| k == key) {
                        mixed.push(key.to_string());
                    }
                }
            }
        }
        let result = json!({ "properties": properties, "values": values, "mixed": mixed });
        *self.properties.cache.borrow_mut() = Some((model, range, result.clone()));
        result
    }
}
