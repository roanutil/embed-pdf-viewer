//! `services/annotation-rows.ts`: the annotations an action can live on, of
//! both read families: a page's annotations (`doc.annotations`,
//! `doc.annotate.read`) and its widgets, which come with the form
//! (`doc.forms.read`). Each family is read only when the session may read
//! it, so a fill-only session still finds its widgets' actions.

use epdf_kernel::Error;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use serde_json::Value;

use super::{all2, annotation_key, get};
use crate::ActionsCapability;

fn annotations_of(listed: &Value) -> Vec<Value> {
    get(listed, "annotations").and_then(Value::as_array).cloned().unwrap_or_default()
}

fn widgets_of(snapshot: &Value) -> Vec<Value> {
    get(snapshot, "widgets").and_then(Value::as_array).cloned().unwrap_or_default()
}

fn page_number_of(annotation: &Value) -> Option<f64> {
    get(annotation, "page").and_then(|p| get(p, "objectNumber")).and_then(Value::as_f64)
}

impl ActionsCapability {
    fn list_page_annotations(&self, pon: u32) -> Promise<Value> {
        match self.doc() {
            Ok(doc) => doc.call("page.annotations.list", Some(pon), Vec::new()),
            Err(error) => Promise::rejected(error),
        }
    }

    /// `readPageAnnotations(ctx, form, page, read)`: every annotation of a
    /// page the session may read, its annotations then its widgets.
    pub(crate) async fn read_page_annotations(
        self,
        page: PageRef,
        signal: Option<AbortSignal>,
    ) -> Result<Vec<Value>, Error> {
        let annotations = if self.allows("doc.annotate.read") {
            let list =
                self.read(signal.as_ref(), self.list_page_annotations(page.page_object_number));
            list.map(|listed| Ok(annotations_of(&listed)))
        } else {
            Promise::resolved(Vec::new())
        };
        let widgets = if self.allows("doc.forms.read") {
            let pon = f64::from(page.page_object_number);
            self.read_form(signal.as_ref()).map(move |snapshot| {
                Ok(widgets_of(&snapshot)
                    .into_iter()
                    .filter(|widget| page_number_of(widget) == Some(pon))
                    .collect::<Vec<_>>())
            })
        } else {
            Promise::resolved(Vec::new())
        };
        let (mut annotations, widgets) = all2(annotations, widgets).await?;
        annotations.extend(widgets);
        Ok(annotations)
    }

    /// `readAnnotation(ctx, form, ref, read)`: the annotation `ref` names,
    /// of either family, or `None` when the session can't see it: the
    /// form's widgets first, then the page's annotations.
    pub(crate) async fn read_annotation(
        self,
        reference: Value,
        signal: Option<AbortSignal>,
    ) -> Result<Option<Value>, Error> {
        let key = annotation_key(&reference);
        let matches = |candidate: &Value| {
            annotation_key(get(candidate, "ref").unwrap_or(&Value::Null)) == key
        };
        if self.allows("doc.forms.read") {
            let snapshot = self.read_form(signal.as_ref()).await?;
            if let Some(widget) = widgets_of(&snapshot).into_iter().find(|w| matches(w)) {
                return Ok(Some(widget));
            }
        }
        if !self.allows("doc.annotate.read") {
            return Ok(None);
        }
        // `ctx.doc.page(ref.page)`: an unreadable page ref still reaches the
        // engine, which refuses it.
        let pon = get(&reference, "page")
            .and_then(|p| get(p, "objectNumber"))
            .and_then(Value::as_f64)
            .map_or(0, |n| n as u32);
        let listed = self.read(signal.as_ref(), self.list_page_annotations(pon)).await?;
        Ok(annotations_of(&listed).into_iter().find(|a| matches(a)))
    }
}
