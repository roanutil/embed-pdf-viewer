//! `services/pictures.ts`: the engine's pictures of each page's widgets,
//! through the render plugin's one door. The form layer draws them, and the
//! annotation plugin draws the widgets it borrows from the same ones
//! (design mode). A page's pictures depend on its widgets' picture versions
//! (the confirmed form's) and nothing else.

use std::rc::Rc;

use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_render::{AppearanceFamily, AppearanceItem, AppearancePicture, AppearancesOptions};
use serde_json::Value;

use crate::model::picture_version_of;
use crate::value::{get, str_of};

impl crate::controller::Core {
    /// `itemsOn(pageObjectNumber)`: the page's widgets as the engine
    /// confirmed them, each at its picture version.
    pub(crate) fn picture_items_on(&self, page: f64) -> Vec<AppearanceItem> {
        let index = self.truth();
        index
            .widgets()
            .iter()
            .filter_map(|row| {
                let reference = get(row, "ref")?;
                let on_page =
                    get(row, "page").and_then(|p| get(p, "objectNumber")).and_then(Value::as_f64)
                        == Some(page);
                if !on_page || str_of(reference, "kind") != Some("objectNumber") {
                    return None;
                }
                let n = get(reference, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
                Some(AppearanceItem {
                    reference: reference.clone(),
                    version: picture_version_of(&index, n),
                })
            })
            .collect()
    }

    /// `getAppearanceEpoch(page)`: what the page's pictures depend on,
    /// `5@2|6@7`.
    pub fn get_appearance_epoch(&self, page: &Value) -> String {
        let page = get(page, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
        let mut parts: Vec<String> = self
            .picture_items_on(page)
            .iter()
            .map(|item| {
                let number = if str_of(&item.reference, "kind") == Some("objectNumber") {
                    crate::value::string_of(get(&item.reference, "objectNumber"))
                } else {
                    String::new()
                };
                format!("{number}@{}", epdf_geometry::js::number_to_string(item.version))
            })
            .collect();
        // `Array#sort` with no comparator: by UTF-16 code units.
        parts.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        parts.join("|")
    }

    /// `renderAppearances(page, scale, signal)`: every picture of the page's
    /// widgets, at rest, in every state; none without the render plugin.
    pub fn render_appearances(
        &self,
        page: &Value,
        scale: f64,
        signal: Option<AbortSignal>,
    ) -> Promise<Rc<Vec<AppearancePicture>>> {
        let Some(render) = self.render() else { return Promise::resolved(Rc::new(Vec::new())) };
        let number = get(page, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
        let items = self.picture_items_on(number);
        let Some(page) = epdf_kernel::geometry::PageRef::from_json(page) else {
            return Promise::resolved(Rc::new(Vec::new()));
        };
        render.render_appearances(
            page,
            AppearancesOptions { family: AppearanceFamily::Widgets, scale, items, signal },
        )
    }

    /// `pictures.versionOf(widget)`: the widget's confirmed picture version.
    pub fn picture_version_of_ref(&self, widget: &Value) -> f64 {
        if str_of(widget, "kind") != Some("objectNumber") {
            return 0.0;
        }
        let n = get(widget, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
        picture_version_of(&self.truth(), n)
    }
}
