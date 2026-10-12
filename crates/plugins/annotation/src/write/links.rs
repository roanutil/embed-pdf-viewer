//! `write/links.ts`: attached links (a Link child riding an editable
//! annotation) and group relationships. The one reconciler that creates,
//! retargets, re-rects and deletes link children, and the relationship-only
//! patch grouping uses.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_annotation_core::{
    Annot, AnnotationRef, Id, Msg, PdfLinkTarget, PdfRect, Source, content_to_pdf_rect,
    link_children_of, link_of,
};
use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};
use serde_json::{Map, Value, json};

use crate::controller::AnnotationCapability;
use crate::promise::{self, await_engine};
use crate::repository::seam::pdf_json;
use crate::repository::{annotation_key, link_child_rects, writable_target};
use crate::value::get;

/// Per-parent reconcile chains, in `Map` insertion order.
#[derive(Default)]
pub(crate) struct LinkChains(RefCell<Vec<(Id, Promise<()>)>>);

/// `scheduleSync`'s intent: an explicit target, or follow whatever the
/// substrate holds at run time.
#[derive(Clone, Debug)]
pub(crate) enum LinkIntent {
    Keep,
    Target(Option<PdfLinkTarget>),
}

fn ref_json(r: &AnnotationRef) -> Value {
    serde_json::to_value(r).unwrap_or_default()
}

/// `annotation.links`: the Link child attached to an annotation.
#[derive(Clone)]
pub struct LinksLens(pub(crate) AnnotationCapability);

impl LinksLens {
    /// `links.get(ref)`.
    pub fn get(&self, r: &AnnotationRef) -> Option<PdfLinkTarget> {
        let m = self.0.model();
        let a = m.by_id.get(&annotation_key(r))?;
        if a.subtype == "link" { a.link.as_option().cloned() } else { link_of(&m, &a.id) }
    }

    /// `links.set(ref, target)`: resolves when the children are committed.
    pub fn set(&self, r: &AnnotationRef, target: PdfLinkTarget) -> Promise<()> {
        self.0.schedule_link_sync(&annotation_key(r), LinkIntent::Target(Some(target)))
    }

    /// `links.clear(ref)`.
    pub fn clear(&self, r: &AnnotationRef) -> Promise<()> {
        self.0.schedule_link_sync(&annotation_key(r), LinkIntent::Target(None))
    }
}

impl AnnotationCapability {
    /// `links`.
    pub fn links(&self) -> LinksLens {
        LinksLens(self.clone())
    }

    /// `scheduleSync(id, intent)`: chain a reconcile after the parent's
    /// previous one; returns the chain.
    pub(crate) fn schedule_link_sync(&self, id: &str, intent: LinkIntent) -> Promise<()> {
        let prev = self
            .0
            .link_chains
            .0
            .borrow()
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, p)| p.clone())
            .unwrap_or_else(|| Promise::resolved(()));
        let this = self.clone();
        let owned = id.to_string();
        let next = Promise::spawn(async move {
            prev.await?;
            let desired = match intent {
                LinkIntent::Keep => link_of(&this.model(), &owned),
                LinkIntent::Target(t) => t,
            };
            let run = this.reconcile_link_children(owned, desired);
            rt::tick().await;
            run.await
        });
        {
            let mut chains = self.0.link_chains.0.borrow_mut();
            match chains.iter_mut().find(|(k, _)| k == id) {
                Some(slot) => slot.1 = next.clone(),
                None => chains.push((id.to_string(), next.clone())),
            }
        }
        let weak = Rc::downgrade(&self.0);
        let (key, mine) = (id.to_string(), next.clone());
        rt::spawn(async move {
            let _ = mine.clone().await;
            let Some(core) = weak.upgrade() else { return };
            let mut chains = core.link_chains.0.borrow_mut();
            if chains.iter().any(|(k, p)| *k == key && p.same(&mine)) {
                chains.retain(|(k, _)| *k != key);
            }
        });
        next
    }

    /// `reconcileChildren(id, desired)`: the one place attached link
    /// children are created, retargeted, re-rected or deleted.
    fn reconcile_link_children(&self, id: Id, desired: Option<PdfLinkTarget>) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let doc = this.doc();
            let a = this.model().by_id.get(&id).cloned();
            let (Some(doc), Some(a)) = (doc, a) else { return Ok(()) };
            let Some(parent_ref) = a.ref_.clone() else { return Ok(()) };
            if a.subtype == "link" {
                return Ok(());
            }
            let pon = a.page.page_object_number;
            let Some(crop) = this.crop_of(pon) else { return Ok(()) };
            doc.page_index(pon)?;
            let target = writable_target(desired.as_ref());
            let rects: Vec<PdfRect> = match desired {
                None => Vec::new(),
                Some(_) => {
                    link_child_rects(&a).into_iter().map(|r| content_to_pdf_rect(r, crop)).collect()
                }
            };
            let current: Vec<Annot> =
                link_children_of(&this.model(), &id).into_iter().cloned().collect();
            let target_json = target.as_ref().map(|t| serde_json::to_value(t).unwrap_or_default());
            let call = |method: &str, args: Vec<Value>| {
                doc.call(&format!("page.annotations.{method}"), Some(pon), args)
            };
            let run = async {
                let paired = current.len().min(rects.len());
                for i in 0..paired {
                    let Some(r) = current[i].ref_.clone() else { continue };
                    let mut patch = Map::new();
                    patch.insert("subtype".into(), json!("link"));
                    patch.insert("rect".into(), pdf_json(rects[i]));
                    if let Some(t) = &target_json {
                        patch.insert("target".into(), t.clone());
                    }
                    let res =
                        await_engine(call("update", vec![ref_json(&r), Value::Object(patch)]))
                            .await?;
                    let dto = get(&res, "updated").cloned().unwrap_or(Value::Null);
                    let annot = this.ingest(&dto, crop, Source::Baked)?;
                    this.commit(Msg::Upsert { annots: vec![annot], bump_ap: None });
                }
                for rect in rects.iter().skip(current.len()) {
                    let draft = json!({
                        "subtype": "link",
                        "rect": pdf_json(*rect),
                        "target": target_json.clone().unwrap_or(Value::Null),
                        "inReplyTo": ref_json(&parent_ref),
                        "replyType": "group",
                    });
                    let res = await_engine(call("create", vec![draft])).await?;
                    let dto = get(&res, "created").cloned().unwrap_or(Value::Null);
                    let annot = this.ingest(&dto, crop, Source::Baked)?;
                    this.commit(Msg::Upsert { annots: vec![annot], bump_ap: None });
                }
                for child in current.iter().skip(rects.len()) {
                    if let Some(r) = &child.ref_ {
                        await_engine(call("delete", vec![ref_json(r)])).await?;
                    }
                    this.commit(Msg::Remove { ids: vec![child.id.clone()] });
                }
                Ok::<(), Error>(())
            };
            if let Err(err) = run.await {
                this.console_error("[annotation] attached-link sync failed:", Some(&err));
            }
            Ok(())
        })
    }

    /// `writeRelationship(a, rel)`: a relationship-only patch, re-synced
    /// with the annotation's own render source.
    pub(crate) fn write_relationship(
        &self,
        a: Annot,
        in_reply_to: Option<AnnotationRef>,
        group: bool,
    ) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let Some(doc) = this.doc() else { return Ok(()) };
            let (Some(r), Some(data)) = (a.ref_.clone(), a.data.clone()) else { return Ok(()) };
            let pon = a.page.page_object_number;
            doc.page_index(pon)?;
            let mut patch = Map::new();
            patch.insert("subtype".into(), get(&data, "subtype").cloned().unwrap_or(Value::Null));
            patch.insert("inReplyTo".into(), in_reply_to.as_ref().map_or(Value::Null, ref_json));
            if group {
                patch.insert("replyType".into(), json!("group"));
            }
            let call = doc.call(
                "page.annotations.update",
                Some(pon),
                vec![ref_json(&r), Value::Object(patch)],
            );
            let res = await_engine(call).await?;
            this.sync(get(&res, "updated").unwrap_or(&Value::Null), a.source, false)?;
            Ok(())
        })
    }

    pub(crate) fn run_sync_link_effect(&self, id: Id, target: Option<PdfLinkTarget>) {
        if self.doc().is_none() {
            return;
        }
        let _ = self.schedule_link_sync(&id, LinkIntent::Target(target));
    }
}
