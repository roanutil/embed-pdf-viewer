//! `working-set.ts`: what this lens shows, told to the engine as the lens's
//! working set: the engine ranks the jobs for those pages by it, and keeps
//! those pages parsed longest. Every lens tells its own.
//!
//! It comes from the pages the lens lays out (`list_visible_pages`): a page
//! with some of it on screen is `visible`, with that part and its device
//! pixels; a page laid out just off screen is `near`.
//!
//! It is told through `ctx.doc.setWorkingSet(id, pages)`
//! ([`epdf_kernel::engine::DocumentHandle::set_working_set`]).

use std::cell::RefCell;
use std::rc::Rc;

use epdf_geometry::{Rect, js};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::{Controller, Error};
use serde::Serialize;
use serde_json::Value;

use crate::contract::VisiblePage;
use crate::controller::StageCapability;
use crate::model::StageState;

/// A page's role in a lens's working set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkingSetRole {
    Visible,
    Near,
}

/// `WorkingSetPage`: one page of a lens's working set.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct WorkingSetPage {
    pub page: PageRef,
    pub role: WorkingSetRole,
    /// The page's part on screen, in page space; absent for a near page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<Rect>,
    /// Device pixels of the visible part; 0 for a near page.
    pub pixels: f64,
}

/// `workingSetOf(pages)`: a lens's working set, from the pages it lays out.
pub fn working_set_of(pages: &[VisiblePage]) -> Vec<WorkingSetPage> {
    pages
        .iter()
        .map(|page| {
            let visible = page.visible_rect;
            if visible.width <= 0.0 || visible.height <= 0.0 {
                return WorkingSetPage {
                    page: page.page_ref,
                    role: WorkingSetRole::Near,
                    visible: None,
                    pixels: 0.0,
                };
            }
            let pixels = visible.width * visible.height * page.transform.render_scale.powi(2);
            WorkingSetPage {
                page: page.page_ref,
                role: WorkingSetRole::Visible,
                visible: Some(visible),
                pixels: js::round(pixels),
            }
        })
        .collect()
}

/// A working set as one comparable string.
fn key_of(pages: &[WorkingSetPage]) -> String {
    let n = |x: f64| js::number_to_string(js::round(x));
    pages
        .iter()
        .map(|page| {
            let part = page.visible.map_or(String::new(), |v| {
                format!("{},{},{},{}", n(v.x), n(v.y), n(v.width), n(v.height))
            });
            let role = match page.role {
                WorkingSetRole::Visible => "visible",
                WorkingSetRole::Near => "near",
            };
            format!(
                "{}:{role}:{}:{part}",
                page.page.page_object_number,
                js::number_to_string(page.pixels)
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// `connectWorkingSet(ctx, stage)`: tell the engine what the lens shows, now
/// and whenever it changes, at most once per camera move.
pub(crate) fn connect_working_set(
    ctx: &Controller<StageState, Value>,
    stage: &StageCapability,
) -> Result<(), Error> {
    let told = Rc::new(RefCell::new(String::new()));
    let tell = {
        let told = told.clone();
        let ctx = ctx.clone();
        move |shown: &[VisiblePage]| -> Result<(), Error> {
            let pages = working_set_of(shown);
            let key = key_of(&pages);
            if *told.borrow() == key {
                return Ok(());
            }
            *told.borrow_mut() = key;
            let pages = serde_json::to_value(&pages).map_err(|e| Error::new(e.to_string()))?;
            ctx.doc()?.set_working_set(ctx.id(), pages);
            Ok(())
        }
    };
    let tell = Rc::new(tell);
    let select = stage.clone();
    let on_change = tell.clone();
    ctx.watch(move || Ok(select.list_visible_pages()), move |next, _| on_change(next), Rc::ptr_eq)?;
    tell(&stage.list_visible_pages())
}
