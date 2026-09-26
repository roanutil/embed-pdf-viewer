//! `read/annotations.ts`: the page-space read model. Every annotation is
//! projected from its model entry (the model already lives in page space).

use epdf_annotation_core::{
    Annot, AnnotationRef, Geom, Id, Model, geom_visual_bounds, props_for, read_prop,
};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use serde_json::{Map, Value};

use crate::contract::*;
use crate::controller::AnnotationCapability;
use crate::repository::annotation_key;
use crate::value::get;

fn geometry_of(g: &Geom) -> AnnotationGeometry {
    match g {
        Geom::Rect { rect, rot, .. } => {
            AnnotationGeometry::Rect { bounds: *rect, rotation: rot.unwrap_or(0.0) }
        }
        Geom::Line { a, b, .. } => AnnotationGeometry::Line { from: *a, to: *b },
        Geom::Poly { points, closed: true, .. } => {
            AnnotationGeometry::Polygon { vertices: points.clone() }
        }
        Geom::Poly { points, .. } => AnnotationGeometry::Polyline { vertices: points.clone() },
        Geom::Ink { strokes, .. } => AnnotationGeometry::Ink { strokes: strokes.clone() },
        Geom::Quads { quads } => AnnotationGeometry::Markup { quads: quads.clone() },
        Geom::Caret { rect, .. } => AnnotationGeometry::Caret { bounds: *rect },
        Geom::Text { rect, rot, callout } => AnnotationGeometry::Text {
            bounds: *rect,
            rotation: rot.unwrap_or(0.0),
            callout: *callout,
        },
    }
}

fn props_of(a: &Annot) -> Map<String, Value> {
    let mut out = Map::new();
    for spec in props_for(&a.subtype) {
        if let Some(value) = read_prop(a, spec.key) {
            out.insert(spec.key.as_str().into(), serde_json::to_value(value).unwrap_or_default());
        }
    }
    out
}

fn member(d: Option<&Value>, key: &str) -> Value {
    d.and_then(|d| get(d, key)).cloned().unwrap_or(Value::Null)
}

/// `projectAnnot(a)`: `None` for an entry the engine has not confirmed.
pub(crate) fn project_annot(m: &Model, a: &Annot) -> Option<Annotation> {
    let ref_ = a.ref_.clone()?;
    let d = a.data.as_ref();
    let contents = d.and_then(|d| get(d, "contents")).filter(|c| !c.is_null());
    Some(Annotation {
        ref_,
        page: a.page,
        subtype: a.subtype.clone(),
        bounds: geom_visual_bounds(&a.geom, a.style.stroke_width, Some(&a.style.border)),
        geometry: geometry_of(&a.geom),
        props: props_of(a),
        flags: a.flags,
        contents: contents.cloned().unwrap_or_else(|| Value::String(String::new())),
        author: member(d, "author"),
        created_at: member(d, "created"),
        modified_at: member(d, "modified"),
        group: a.group.as_ref().and_then(|g| m.by_id.get(g)).and_then(|p| p.ref_.clone()),
        in_reply_to: member(d, "inReplyTo"),
        authority: a.authority.map_or(RecordAuthority { update: true, delete: true }, |x| {
            RecordAuthority { update: x.update, delete: x.delete }
        }),
        raw: d.cloned(),
    })
}

impl AnnotationCapability {
    /// `projectRef(ref)`.
    pub(crate) fn project_ref(&self, r: &AnnotationRef) -> Option<Annotation> {
        let m = self.model();
        let a = m.by_id.get(&annotation_key(r))?;
        project_annot(&m, a)
    }

    /// `listAnnots(filter)`: model entries in z-order, every field narrowing.
    pub(crate) fn list_annots(&self, filter: Option<&AnnotationFilter>) -> Vec<Annot> {
        let m = self.model();
        let group = filter.and_then(|f| f.group.as_ref()).map(annotation_key);
        m.order
            .iter()
            .filter_map(|id| m.by_id.get(id))
            .filter(|a| {
                let Some(f) = filter else { return true };
                f.page.is_none_or(|p| a.page.same_page(&p))
                    && f.subtype.as_ref().is_none_or(|s| a.subtype == *s)
                    && f.author.as_ref().is_none_or(|author| {
                        a.data.as_ref().and_then(|d| get(d, "author")).and_then(Value::as_str)
                            == Some(author.as_str())
                    })
                    && group.as_ref().is_none_or(|g| a.group.as_ref() == Some(g) || a.id == *g)
            })
            .cloned()
            .collect()
    }

    /// `loadedOrThrow(ref)`: the committed entry, or `not-found`.
    pub(crate) fn loaded_or_throw(&self, r: &AnnotationRef) -> Result<Annot, Error> {
        match self.model().by_id.get(&annotation_key(r)) {
            Some(a) if a.ref_.is_some() => Ok(a.clone()),
            _ => Err(plugin_error(
                code::NOT_FOUND,
                "annotation",
                "annotation is not loaded in this document",
                None,
                None,
            )),
        }
    }

    /// `selectedCommitted()`: committed, data-backed members of the selection.
    pub(crate) fn selected_committed(&self) -> Vec<Annot> {
        let m = self.model();
        m.selected
            .iter()
            .filter_map(|id| m.by_id.get(id))
            .filter(|a| a.ref_.is_some() && a.data.is_some())
            .cloned()
            .collect()
    }

    /// `get(ref)`: the page-space record, or `None`.
    pub fn get(&self, r: &AnnotationRef) -> Option<Annotation> {
        self.project_ref(r)
    }

    /// `list(filter)`: records in z-order.
    pub fn list(&self, filter: Option<&AnnotationFilter>) -> Vec<Annotation> {
        let m = self.model();
        self.list_annots(filter).iter().filter_map(|a| project_annot(&m, a)).collect()
    }

    /// `listRaw(filter)`: the engine records.
    pub fn list_raw(&self, filter: Option<&AnnotationFilter>) -> Vec<Value> {
        self.list_annots(filter).into_iter().filter_map(|a| a.data).collect()
    }

    /// `getRaw(ref)`: the engine record, or `None`.
    pub fn get_raw(&self, r: &AnnotationRef) -> Option<Value> {
        self.model().by_id.get(&annotation_key(r))?.data.clone()
    }

    /// `listSelected()`.
    pub fn list_selected(&self) -> Vec<Annotation> {
        let m = self.model();
        m.selected
            .iter()
            .filter_map(|id| m.by_id.get(id))
            .filter_map(|a| project_annot(&m, a))
            .collect()
    }

    /// `getSelection()`: the selection as durable refs.
    pub fn get_selection(&self) -> Vec<AnnotationRef> {
        let m = self.model();
        m.selected.iter().filter_map(|id: &Id| m.by_id.get(id)?.ref_.clone()).collect()
    }
}
