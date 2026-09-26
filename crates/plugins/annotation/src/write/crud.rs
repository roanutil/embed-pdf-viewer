//! `write/crud.ts`: create, update and delete. The public page-space verbs,
//! their raw (PDF-space) twins, and the effect runners that commit every
//! gesture's optimistic change to the engine and re-sync from its confirmed
//! record.

use std::rc::Rc;

use epdf_annotation_core::{
    Annot, AnnotationRef, Geom, Id, Model, Msg, PageRef, PatchScope, Rect, Source, apply_props,
    link_children_of,
};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::{self, Promise};
use serde_json::{Map, Value, json};

use crate::contract::*;
use crate::controller::AnnotationCapability;
use crate::create_input::{CreateAnnotationInput, geometry_from_input};
use crate::promise::{self, await_engine, then};
use crate::repository::{annotation_key, to_create_draft, to_patch, to_scoped_patch};
use crate::services::events::{origin_api, origin_unknown};
use crate::services::write_sink::create_effects_of;
use crate::value::{get, str_of, truthy};
use crate::write::links::LinkIntent;

fn invalid(message: impl Into<String>) -> Error {
    plugin_error(code::INVALID_INPUT, "annotation", message.into(), None, None)
}

fn not_found(message: impl Into<String>) -> Error {
    plugin_error(code::NOT_FOUND, "annotation", message.into(), None, None)
}

/// `{ ...a, ...b }` over two `/F` patches.
fn spread_flags(
    a: Option<epdf_annotation_core::AnnotationFlagsPatch>,
    b: Option<epdf_annotation_core::AnnotationFlagsPatch>,
) -> epdf_annotation_core::AnnotationFlagsPatch {
    let mut out = match serde_json::to_value(a.unwrap_or_default()) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    };
    if let Ok(Value::Object(m)) = serde_json::to_value(b.unwrap_or_default()) {
        out.extend(m);
    }
    serde_json::from_value(Value::Object(out)).unwrap_or_default()
}

fn ref_json(r: &AnnotationRef) -> Value {
    serde_json::to_value(r).unwrap_or_default()
}

fn ref_from(v: &Value) -> Option<AnnotationRef> {
    serde_json::from_value(v.clone()).ok()
}

fn wire_subtype_of(a: &Annot) -> Value {
    a.data.as_ref().and_then(|d| get(d, "subtype")).cloned().unwrap_or_else(|| json!(a.subtype))
}

fn geom_name(g: &Geom) -> &'static str {
    match g {
        Geom::Rect { .. } => "rect",
        Geom::Line { .. } => "line",
        Geom::Poly { .. } => "poly",
        Geom::Quads { .. } => "quads",
        Geom::Caret { .. } => "caret",
        Geom::Ink { .. } => "ink",
        Geom::Text { .. } => "text",
    }
}

/// `geomFromPatch(geom, patch)`: a page-space geometry patch over the
/// model geometry of the same shape.
fn geom_from_patch(geom: &Geom, patch: &AnnotationGeometryPatch) -> Result<Geom, Error> {
    let bad = || {
        invalid(format!(
            "a '{}' geometry cannot replace a '{}' geometry",
            patch.kind(),
            geom_name(geom)
        ))
    };
    let mut g = geom.clone();
    match (patch, &mut g) {
        (AnnotationGeometryPatch::Rect { bounds, rotation }, Geom::Rect { rect, rot, .. }) => {
            *rect = *bounds;
            if let Some(r) = rotation {
                *rot = Some(*r);
            }
        }
        (AnnotationGeometryPatch::Line { from, to }, Geom::Line { a, b, .. }) => {
            *a = *from;
            *b = *to;
        }
        (
            AnnotationGeometryPatch::Polygon { vertices }
            | AnnotationGeometryPatch::Polyline { vertices },
            Geom::Poly { points, closed, .. },
        ) => {
            *points = vertices.clone();
            *closed = matches!(patch, AnnotationGeometryPatch::Polygon { .. });
        }
        (AnnotationGeometryPatch::Ink { strokes: s }, Geom::Ink { strokes, .. }) => {
            *strokes = s.clone();
        }
        (AnnotationGeometryPatch::Markup { quads: q }, Geom::Quads { quads }) => *quads = q.clone(),
        (
            AnnotationGeometryPatch::Text { bounds, rotation, callout: c },
            Geom::Text { rect, rot, callout },
        ) => {
            *rect = *bounds;
            if let Some(r) = rotation {
                *rot = Some(*r);
            }
            if !c.is_undefined() {
                *callout = c.as_option().copied();
            }
        }
        _ => return Err(bad()),
    }
    Ok(g)
}

/// `withBounds(geom, bounds)`.
fn with_bounds(geom: &Geom, bounds: Rect) -> Result<Geom, Error> {
    let mut g = geom.clone();
    match &mut g {
        Geom::Rect { rect, .. } | Geom::Text { rect, .. } | Geom::Caret { rect, .. } => {
            *rect = bounds;
            Ok(g)
        }
        _ => Err(invalid(format!(
            "'{}' geometry has no bounds to set; patch its geometry",
            geom_name(geom)
        ))),
    }
}

/// `withRotation(geom, rot)`.
fn with_rotation(geom: &Geom, degrees: f64) -> Result<Geom, Error> {
    let mut g = geom.clone();
    match &mut g {
        Geom::Quads { .. } | Geom::Caret { .. } => Err(plugin_error(
            code::UNSUPPORTED,
            "annotation",
            format!("'{}' annotations do not rotate", geom_name(geom)),
            None,
            None,
        )),
        Geom::Rect { rot, .. }
        | Geom::Line { rot, .. }
        | Geom::Poly { rot, .. }
        | Geom::Ink { rot, .. }
        | Geom::Text { rot, .. } => {
            *rot = Some(degrees);
            Ok(g)
        }
    }
}

/// `rotationOf(a)`.
pub(crate) fn rotation_of(a: &Annot) -> f64 {
    match &a.geom {
        Geom::Rect { rot, .. }
        | Geom::Line { rot, .. }
        | Geom::Poly { rot, .. }
        | Geom::Ink { rot, .. }
        | Geom::Caret { rot, .. }
        | Geom::Text { rot, .. } => rot.unwrap_or(0.0),
        Geom::Quads { .. } => 0.0,
    }
}

impl AnnotationCapability {
    /// `doc.page(pon).annotations.<method>(...)`: `page(ref)` refuses an
    /// unknown page synchronously.
    pub(crate) fn page_call(
        &self,
        pon: u32,
        method: &str,
        args: Vec<Value>,
    ) -> Result<Promise<Value>, Error> {
        let doc = self.doc().ok_or_else(|| Error::new("[annotation] no document bound"))?;
        doc.page_index(pon)?;
        Ok(doc.call(&format!("page.annotations.{method}"), Some(pon), args))
    }

    /// `updateRaw(ref, patch)`: the one engine-update path. A programmatic
    /// patch changes the appearance, so it renders live.
    pub fn update_raw(&self, r: AnnotationRef, patch: Value) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            if this.doc().is_none() {
                return Err(Error::new("[annotation] no document bound"));
            }
            let pon = this.page_of(&r);
            let call = this.page_call(pon, "update", vec![ref_json(&r), patch])?;
            let res = await_engine(call).await?;
            let updated = get(&res, "updated").cloned().unwrap_or(Value::Null);
            let changed = get(&res, "appearance").and_then(|a| get(a, "changed"));
            this.sync(&updated, Source::Vector, truthy(changed));
            this.announce_updated(&updated, origin_api());
            Ok(())
        })
    }

    /// `update(ref, patch)`: lower a page-space patch through the repository
    /// projection and one engine write; rich text commits through the text
    /// door.
    pub fn update(&self, r: AnnotationRef, patch: AnnotationPatch) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let a = this.loaded_or_throw(&r)?;
            let crop = this
                .crop_of(a.page.page_object_number)
                .ok_or_else(|| not_found("the annotation page is not loaded"))?;
            let mut modified = a.clone();
            let mut touched = false;
            if let Some(bounds) = patch.bounds {
                modified.geom = with_bounds(&modified.geom, bounds)?;
                touched = true;
            }
            if let Some(geometry) = &patch.geometry {
                modified.geom = geom_from_patch(&modified.geom, geometry)?;
                touched = true;
            }
            if let Some(props) = &patch.props {
                modified = apply_props(&modified, props)
                    .ok_or_else(|| invalid("the props patch does not apply to this annotation"))?;
                touched = true;
            }
            let mut engine = Map::new();
            if touched {
                engine.extend(to_patch(&modified, crop).unwrap_or_default());
            }
            if let Some(flags) = &patch.flags {
                let merged = epdf_annotation_core::merge_flags(&a.flags, flags);
                engine.insert("flags".into(), serde_json::to_value(merged).unwrap_or_default());
            }
            if let Some(contents) = &patch.contents {
                engine.insert("contents".into(), json!(contents));
            }
            if !engine.is_empty() {
                let mut full = Map::new();
                full.insert("subtype".into(), wire_subtype_of(&a));
                full.extend(engine);
                this.update_raw(a.ref_.clone().expect("loaded"), Value::Object(full)).await?;
            }
            if let Some(rich) = patch.rich_text {
                this.commit(Msg::SetRichText {
                    id: a.id.clone(),
                    doc: epdf_annotation_core::richtext::RichTextDocumentInput {
                        body: None,
                        paragraphs: rich.paragraphs,
                    },
                });
                if let Some(write) = this.commit_text(a.ref_.as_ref().expect("loaded")) {
                    write.await?;
                }
            }
            Ok(())
        })
    }

    /// `setRotation(ref, degrees)`: the geometry scope.
    pub fn set_rotation(&self, r: AnnotationRef, degrees: f64) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let a = this.loaded_or_throw(&r)?;
            let crop = this
                .crop_of(a.page.page_object_number)
                .ok_or_else(|| not_found("the annotation page is not loaded"))?;
            let mut modified = a.clone();
            modified.geom = with_rotation(&a.geom, degrees)?;
            if let Some(patch) = to_scoped_patch(&modified, &PatchScope::Geometry, crop) {
                this.update_raw(a.ref_.clone().expect("loaded"), Value::Object(patch)).await?;
            }
            Ok(())
        })
    }

    /// `rotateBy(ref, delta)`.
    pub fn rotate_by(&self, r: AnnotationRef, delta: f64) -> Promise<()> {
        match self.loaded_or_throw(&r) {
            Ok(a) => self.set_rotation(r, rotation_of(&a) + delta),
            Err(error) => Promise::rejected(error),
        }
    }

    /// `create(input)`: stage page-space input through the same
    /// `createAnnot` path a draw tool takes; resolves the engine's ref.
    pub fn create(&self, input: CreateAnnotationInput) -> Promise<AnnotationRef> {
        if !self.can_create() {
            return Promise::rejected(plugin_error(
                code::PERMISSION_DENIED,
                "annotation",
                "create requires doc.annotate.create",
                None,
                None,
            ));
        }
        if !self.has_page(input.page.page_object_number) {
            return Promise::rejected(not_found(format!(
                "page {} is not in this document",
                input.page.page_object_number
            )));
        }
        let (subtype, geom) = match geometry_from_input(&input) {
            Ok(staged) => staged,
            Err(error) => return Promise::rejected(error),
        };
        let tool = input.tool.as_deref().and_then(|t| self.tool(t));
        if let Some(t) = &input.tool
            && tool.is_none()
        {
            return Promise::rejected(not_found(format!("unknown tool '{t}'")));
        }
        let flags = spread_flags(tool.as_ref().and_then(|t| t.flags), input.flags);
        let effects = self.commit(Msg::CreateAnnot {
            page: input.page,
            subtype,
            geom,
            preset: tool.as_ref().map(|t| t.preset.clone()).or(input.tool.clone()),
            props: input.props,
            flags: Some(flags),
            select: Some(input.select.unwrap_or(false)),
        });
        self.await_create(create_effects_of(&effects).into_iter().next())
    }

    /// `createMany(inputs)`: per-input outcomes folded into a batch result.
    /// A failed input is reported as the JSON it arrived as.
    pub fn create_many(&self, inputs: Vec<Value>) -> Promise<BatchResult<Value>> {
        self.batch_over(
            inputs,
            |this, input| match serde_json::from_value::<CreateAnnotationInput>(input) {
                Ok(input) => this.create(input),
                Err(e) => Promise::rejected(Error::named("TypeError", e.to_string())),
            },
            |_, out| Some(out),
        )
    }

    /// `createRaw(page, draft)`: the PDF-space escape hatch. An absent `/F`
    /// defaults to `print`.
    pub fn create_raw(&self, page: PageRef, draft: Value) -> Promise<AnnotationRef> {
        let this = self.clone();
        promise::spawn(async move {
            if this.doc().is_none() {
                return Err(Error::new("[annotation] no document bound"));
            }
            let draft = if truthy(get(&draft, "flags")) {
                draft
            } else {
                let mut d = draft.as_object().cloned().unwrap_or_default();
                d.insert("flags".into(), json!({ "print": true }));
                Value::Object(d)
            };
            let call = this.page_call(page.page_object_number, "create", vec![draft])?;
            let res = await_engine(call).await?;
            let created = get(&res, "created").cloned().unwrap_or(Value::Null);
            let source = if str_of(&created, "subtype") == Some("stamp") {
                Source::Baked
            } else {
                Source::Vector
            };
            this.sync(&created, source, false);
            this.announce_created(&created, origin_api());
            get(&created, "ref").and_then(ref_from).ok_or_else(|| {
                Error::named("TypeError", "Cannot read properties of undefined (reading 'ref')")
            })
        })
    }

    /// `delete(ref)`.
    pub fn delete(&self, r: AnnotationRef) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            if this.doc().is_none() {
                return Err(Error::new("[annotation] no document bound"));
            }
            let pon = this.page_of(&r);
            let call = this.page_call(pon, "delete", vec![ref_json(&r)])?;
            await_engine(call).await?;
            this.commit(Msg::Remove { ids: vec![annotation_key(&r)] });
            this.announce_deleted(r, PageRef::new(pon), origin_api());
            Ok(())
        })
    }

    /// `updateMany(refs, patch)`.
    pub fn update_many(
        &self,
        refs: Vec<AnnotationRef>,
        patch: AnnotationPatch,
    ) -> Promise<BatchResult<AnnotationRef>> {
        self.batch_over(refs, move |this, r| this.update(r, patch.clone()), |r, _| Some(r.clone()))
    }

    /// `deleteMany(refs)`.
    pub fn delete_many(&self, refs: Vec<AnnotationRef>) -> Promise<BatchResult<AnnotationRef>> {
        self.batch_over(refs, |this, r| this.delete(r), |r, _| Some(r.clone()))
    }

    // ── the effect runners ────────────────────────────────────────────────────

    pub(crate) fn run_create_effect(&self, id: Id, m: &Rc<Model>) {
        let Some(doc) = self.doc() else { return };
        let Some(a) = m.by_id.get(&id).cloned() else { return };
        let Some(crop) = self.crop_of(a.page.page_object_number) else { return };
        let Some(draft) = to_create_draft(&a, crop, &|| self.random_uuid()) else { return };
        let pon = a.page.page_object_number;
        if doc.page_index(pon).is_err() {
            return;
        }
        let call = doc.call("page.annotations.create", Some(pon), vec![Value::Object(draft)]);
        let this = self.clone();
        then(call, move |result| match result {
            Ok(res) => {
                let created = get(&res, "created").cloned().unwrap_or(Value::Null);
                let Some(r) = get(&created, "ref").and_then(ref_from) else { return };
                this.commit(Msg::Created {
                    temp_id: id.clone(),
                    id: annotation_key(&r),
                    ref_: r.clone(),
                });
                this.sync(&created, Source::Vector, false);
                let origin =
                    if this.is_create_awaited(&id) { origin_api() } else { origin_unknown() };
                this.announce_created(&created, origin);
                this.confirm_create(&id, r);
            }
            Err(error) => {
                this.commit(Msg::CreateFailed { temp_id: id.clone() });
                this.fail_create(&id, &error);
            }
        });
    }

    pub(crate) fn run_create_group_effect(&self, primary: Id, members: Vec<Id>, m: &Rc<Model>) {
        let Some(doc) = self.doc() else { return };
        let mut ids = vec![primary.clone()];
        ids.extend(members.iter().cloned());
        let annots: Vec<Option<Annot>> = ids.iter().map(|id| m.by_id.get(id).cloned()).collect();
        let Some(first) = annots[0].clone() else {
            self.commit(Msg::Remove { ids });
            return;
        };
        let pon = first.page.page_object_number;
        if annots.iter().any(|a| a.as_ref().is_none_or(|a| a.page.page_object_number != pon)) {
            self.commit(Msg::Remove { ids });
            return;
        }
        let crop = self.crop_of(pon);
        let drafts: Vec<Option<Map<String, Value>>> = match crop {
            Some(crop) => annots
                .iter()
                .map(|a| a.as_ref().and_then(|a| to_create_draft(a, crop, &|| self.random_uuid())))
                .collect(),
            None => annots.iter().map(|_| None).collect(),
        };
        if drafts.iter().any(Option::is_none) {
            self.commit(Msg::Remove { ids });
            return;
        }
        let drafts: Vec<Map<String, Value>> = drafts.into_iter().flatten().collect();
        let this = self.clone();
        rt::spawn(async move {
            let mut committed: Vec<(Id, AnnotationRef)> = Vec::new();
            let create = |draft: Map<String, Value>| -> Result<Promise<Value>, Error> {
                doc.page_index(pon)?;
                Ok(doc.call("page.annotations.create", Some(pon), vec![Value::Object(draft)]))
            };
            let read_ref = |created: &Value| {
                get(created, "ref").and_then(ref_from).ok_or_else(|| {
                    Error::named("TypeError", "Cannot read properties of undefined (reading 'ref')")
                })
            };
            let run = async {
                let result = await_engine(create(drafts[0].clone())?).await?;
                let created = get(&result, "created").cloned().unwrap_or(Value::Null);
                let primary_ref = read_ref(&created)?;
                committed.push((primary.clone(), primary_ref.clone()));
                this.commit(Msg::Created {
                    temp_id: primary.clone(),
                    id: annotation_key(&primary_ref),
                    ref_: primary_ref.clone(),
                });
                this.sync(&created, Source::Vector, false);
                for (i, temp_id) in members.iter().enumerate() {
                    let mut draft = drafts[i + 1].clone();
                    draft.insert("inReplyTo".into(), ref_json(&primary_ref));
                    draft.insert("replyType".into(), json!("group"));
                    let result = await_engine(create(draft)?).await?;
                    let created = get(&result, "created").cloned().unwrap_or(Value::Null);
                    let r = read_ref(&created)?;
                    committed.push((temp_id.clone(), r.clone()));
                    this.commit(Msg::Created {
                        temp_id: temp_id.clone(),
                        id: annotation_key(&r),
                        ref_: r,
                    });
                    this.sync(&created, Source::Vector, false);
                }
                this.confirm_create(&primary, primary_ref);
                Ok::<(), Error>(())
            };
            if let Err(error) = run.await {
                let mut remove_ids: Vec<Id> = ids
                    .iter()
                    .filter(|id| !committed.iter().any(|(t, _)| t == *id))
                    .cloned()
                    .collect();
                for (_, r) in committed.iter().rev() {
                    let deleted = match doc.page_index(pon) {
                        Ok(_) => await_engine(doc.call(
                            "page.annotations.delete",
                            Some(pon),
                            vec![ref_json(r)],
                        ))
                        .await
                        .is_ok(),
                        Err(_) => false,
                    };
                    if deleted {
                        remove_ids.push(annotation_key(r));
                    }
                }
                if !remove_ids.is_empty() {
                    this.commit(Msg::Remove { ids: remove_ids });
                }
                this.fail_create(&primary, &error);
                this.console_error(
                    "[annotation] grouped annotation creation failed:",
                    Some(&error),
                );
            }
        });
    }

    pub(crate) fn run_flags_effect(&self, id: Id, m: &Rc<Model>) {
        let Some(doc) = self.doc() else { return };
        let Some(a) = m.by_id.get(&id).cloned() else { return };
        let (Some(r), Some(data)) = (a.ref_.clone(), a.data.clone()) else { return };
        let pon = a.page.page_object_number;
        if doc.page_index(pon).is_err() {
            return;
        }
        let patch = json!({
            "subtype": get(&data, "subtype").cloned().unwrap_or(Value::Null),
            "flags": serde_json::to_value(a.flags).unwrap_or_default(),
        });
        let write = doc.call("page.annotations.update", Some(pon), vec![ref_json(&r), patch]);
        self.note_write(Some(r), write.clone());
        let this = self.clone();
        then(write, move |result| match result {
            Ok(res) => {
                let updated = get(&res, "updated").cloned().unwrap_or(Value::Null);
                this.sync(&updated, a.source, false);
                this.announce_updated(&updated, origin_unknown());
            }
            Err(err) => this.console_error("[annotation] flags write failed:", Some(&err)),
        });
    }

    pub(crate) fn run_patch_effect(&self, id: Id, scope: PatchScope, m: &Rc<Model>) {
        let Some(doc) = self.doc() else { return };
        let Some(a) = m.by_id.get(&id).cloned() else { return };
        let crop = self.crop_of(a.page.page_object_number);
        let patch = match (&a.ref_, crop) {
            (Some(_), Some(crop)) => to_scoped_patch(&a, &scope, crop),
            _ => None,
        };
        let (Some(r), Some(patch)) = (a.ref_.clone(), patch) else { return };
        let pon = a.page.page_object_number;
        if doc.page_index(pon).is_err() {
            return;
        }
        let write = doc.call(
            "page.annotations.update",
            Some(pon),
            vec![ref_json(&r), Value::Object(patch)],
        );
        self.note_write(Some(r), write.clone());
        let this = self.clone();
        then(write, move |result| match result {
            Ok(res) => {
                let updated = get(&res, "updated").cloned().unwrap_or(Value::Null);
                let changed = get(&res, "appearance").and_then(|ap| get(ap, "changed"));
                this.sync(&updated, a.source, truthy(changed));
                this.announce_updated(&updated, origin_unknown());
                if !link_children_of(&this.model(), &id).is_empty() {
                    let _ = this.schedule_link_sync(&id, LinkIntent::Keep);
                }
            }
            Err(_) => {
                if let (Some(data), Some(crop)) = (&a.data, crop) {
                    let annot = this.ingest(data, crop, a.source);
                    this.commit(Msg::Upsert { annots: vec![annot], bump_ap: None });
                }
            }
        });
    }

    pub(crate) fn run_delete_effect(&self, r: AnnotationRef) {
        let Some(doc) = self.doc() else { return };
        let page = match &r {
            AnnotationRef::ObjectNumber { page, .. }
            | AnnotationRef::Nm { page, .. }
            | AnnotationRef::Index { page, .. } => *page,
        };
        if doc.page_index(page.page_object_number).is_err() {
            return;
        }
        let write =
            doc.call("page.annotations.delete", Some(page.page_object_number), vec![ref_json(&r)]);
        self.note_write(Some(r.clone()), write.clone());
        let this = self.clone();
        then(write, move |result| {
            if result.is_ok() {
                this.announce_deleted(r, page, origin_unknown());
            }
        });
    }
}
