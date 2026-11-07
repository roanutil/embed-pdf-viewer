//! `read/properties.ts`: the selection's style-panel properties, memoized by
//! model identity. Only the members people may change by hand count.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_annotation_core::editable::is_editable_by_hand;
use epdf_annotation_core::links::link_of;
use epdf_annotation_core::props::selection_properties;
use epdf_annotation_core::record::text_of;
use epdf_annotation_core::richtext::{RichTextRange, rich_text_length};
use epdf_annotation_core::types::{Model, ModelAnnotation, TextStyle};
use serde_json::{Value, json};

use crate::controller::Core;
use crate::js::Object;
use crate::read::property_values::{PropertySources, property_value, property_values};
use crate::rich_text::{RANGE_KEYS, TextSelection, caret_props, range_props, rich_doc_of};

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
    /// `editorSelection(model)`: the editor's selection inside the annotation
    /// being edited, or none (no editor selection, or one left behind by a
    /// previous edit).
    pub(crate) fn editor_selection(&self, model: &Model) -> Option<Rc<TextSelection>> {
        let ts = self.state().text_selection.clone()?;
        (model.editing.as_deref() == Some(ts.id.as_str()) && model.by_id.contains_key(&ts.id))
            .then_some(ts)
    }

    /// `activeTextRange(model)`: the editor's text range inside the
    /// annotation being edited, or none (no selection, or a bare caret).
    pub(crate) fn active_text_range(&self, model: &Model) -> Option<Rc<TextSelection>> {
        self.editor_selection(model).filter(|ts| ts.end > ts.start)
    }

    /// `activeCaret(model)`: the editor's caret inside the annotation being
    /// edited, or none. The range properties there are the text typed next.
    pub(crate) fn active_caret(&self, model: &Model) -> Option<Rc<TextSelection>> {
        self.editor_selection(model).filter(|ts| ts.end == ts.start)
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

    /// `textPropsOf(model, record)`: what a free text's range properties
    /// read: the editor's range, the text typed next at its caret, else all
    /// of the box's text. `None` for any other kind.
    fn text_props_of(
        &self,
        model: &Model,
        record: &ModelAnnotation,
    ) -> Option<(Object, Vec<String>)> {
        if record.annotation.get("subtype").and_then(Value::as_str) != Some("free-text") {
            return None;
        }
        let fonts = self.fonts();
        let doc = rich_doc_of(&record.annotation, fonts.as_deref());
        let props = match self.editor_selection(model).filter(|ts| ts.id == record.id) {
            None => {
                let length = rich_text_length(&doc.typed());
                range_props(&doc, RichTextRange { start: 0.0, end: length }, fonts.as_deref())
            }
            Some(ts) if ts.end > ts.start => {
                range_props(&doc, RichTextRange { start: ts.start, end: ts.end }, fonts.as_deref())
            }
            Some(ts) => caret_props(&doc, ts.start, ts.typing.as_ref(), fonts.as_deref()),
        };
        // A run whose color the body lacks reads as TypeScript's TypeError;
        // the panel shows nothing then.
        props.ok()
    }

    /// `selectionPropertiesOf()`: the same object while nothing changes.
    pub(crate) fn selection_properties(&self) -> Value {
        let model = self.model();
        let selection = self.editor_selection(&model);
        if let Some((m, r, v)) = &*self.properties.cache.borrow()
            && Rc::ptr_eq(m, &model)
            && opt_ptr(r, &selection)
        {
            return v.clone();
        }
        let members: Vec<&Rc<ModelAnnotation>> = model
            .selected
            .iter()
            .filter_map(|id| model.by_id.get(id))
            .filter(|r| is_editable_by_hand(&model.editable, &r.annotation))
            .collect();
        let annotations: Vec<&Object> = members.iter().map(|r| &*r.annotation).collect();
        let properties = selection_properties(&model.editable, &annotations);
        let sources: Vec<Sources> = members.iter().map(|r| self.sources_of(&model, r)).collect();
        // A free text's range properties are its text's: what the next change
        // styles reads back, resolved against the body.
        let texts: Vec<Option<(Object, Vec<String>)>> =
            members.iter().map(|r| self.text_props_of(&model, r)).collect();
        let key_of =
            |spec: &Value| spec.get("key").and_then(Value::as_str).unwrap_or("").to_string();
        let value_of = |spec: &Value, index: usize| -> Option<Value> {
            let key = key_of(spec);
            match &texts[index] {
                Some((values, _)) if RANGE_KEYS.contains(&key.as_str()) => {
                    values.get(&key).cloned().filter(|v| !epdf_kernel::is_undefined(v))
                }
                _ => property_value(spec, &sources[index].view()),
            }
        };
        let mut values = match sources.first() {
            Some(first) => property_values(&properties, &first.view()),
            None => json!({}),
        };
        let mut mixed: Vec<String> = Vec::new();
        for spec in &properties {
            if sources.is_empty() {
                break;
            }
            let key = key_of(spec);
            let of_text = RANGE_KEYS.contains(&key.as_str());
            if of_text && let Some((text_values, _)) = &texts[0] {
                values[key.as_str()] =
                    text_values.get(&key).cloned().unwrap_or_else(epdf_kernel::undefined);
            }
            let first = crate::js::stringify(value_of(spec, 0).as_ref());
            if (0..sources.len()).any(|i| crate::js::stringify(value_of(spec, i).as_ref()) != first)
                || (of_text
                    && texts
                        .iter()
                        .any(|t| t.as_ref().is_some_and(|(_, m)| m.iter().any(|k| *k == key))))
            {
                mixed.push(key);
            }
        }
        let result = json!({ "properties": properties, "values": values, "mixed": mixed });
        *self.properties.cache.borrow_mut() = Some((model, selection, result.clone()));
        result
    }
}
