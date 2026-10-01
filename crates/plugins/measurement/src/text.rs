//! The measurement slice's JSON text, written a page at a time.
//!
//! The slice holds an entry for every page, and a scroll that brings pages
//! into view replaces one entry per page whose scale loads: on a 400-page
//! document that rebuilt and serialized all 400 entries, about 80 KB, for
//! each page. Entries are shared (`Rc<PageEntry>`), so an entry the last
//! state also held keeps the text it was written as, and only a replaced one
//! is serialized again. The text is byte for byte
//! `js::to_value(state).to_string()`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::js;

use crate::model::{MeasurementState, PageEntry};

type Written = HashMap<*const PageEntry, (Rc<PageEntry>, Rc<str>)>;

/// The slice text writer `slice_text` registers. Its cache holds the entries
/// of the last state it wrote, and the text of each.
pub(crate) fn measurement_text() -> impl Fn(&MeasurementState) -> String {
    let written: RefCell<Written> = RefCell::default();
    move |state| {
        let mut last = written.borrow_mut();
        let mut next: Written = HashMap::with_capacity(state.pages.len());
        let mut out = String::with_capacity(64 + 256 * state.pages.len());
        out.push_str("{\"pages\":{");
        for (i, (page, entry)) in state.pages.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let at = Rc::as_ptr(entry);
            let text = match last.remove(&at).or_else(|| next.get(&at).cloned()) {
                Some((_, text)) => text,
                None => js::to_value(&**entry).to_string().into(),
            };
            out.push('"');
            out.push_str(&page.to_string());
            out.push_str("\":");
            out.push_str(&text);
            next.insert(at, (entry.clone(), text));
        }
        *last = next;
        out.push_str("},\"pending\":");
        out.push_str(&js::to_value(&state.pending).to_string());
        out.push_str(",\"calibration\":");
        out.push_str(&js::to_value(&state.calibration).to_string());
        out.push_str(",\"reports\":");
        out.push_str(&js::to_value(&state.reports).to_string());
        out.push('}');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two entries from a real 400-page scroll: page 5 with its scale loaded
    /// (the f32-widened conversion, `m²`), page 12 still on the default.
    const STATE: &str = r#"{"pages":{
        "5":{"viewports":[],"scale":{"measure":{"subtype":"RL","ratio":"1:1",
            "x":[{"unit":"m","conversion":0.00035277777351439,"precision":100.0}],
            "distance":[{"unit":"m","conversion":1.0,"precision":100.0}],
            "area":[{"unit":"m²","conversion":1.0,"precision":100.0}]},
            "source":"default","ready":true,"persistent":true}},
        "12":{"viewports":[],"scale":{"measure":null,"source":"default","ready":false,"persistent":true}}
    },"pending":2.0,"calibration":null,"reports":[]}"#;

    fn whole(state: &MeasurementState) -> String {
        js::to_value(state).to_string()
    }

    #[test]
    fn writes_what_the_whole_value_serializes_to() {
        let text = measurement_text();
        let mut state: MeasurementState = serde_json::from_str(STATE).unwrap();
        assert_eq!(text(&state), whole(&state));
        assert_eq!(text(&MeasurementState::default()), whole(&MeasurementState::default()));
        state.pending = f64::NAN;
        assert_eq!(text(&state), whole(&state));
    }

    #[test]
    fn reuses_shared_entries_and_rewrites_replaced_ones() {
        let text = measurement_text();
        let state: MeasurementState = serde_json::from_str(STATE).unwrap();
        assert_eq!(text(&state), whole(&state));
        // A dispatch copies the map's pointers and replaces one entry.
        let mut next = state.clone();
        let loaded = state.pages[&5].clone();
        next.pages.insert(12, Rc::new((*loaded).clone()));
        next.pages.insert(30, loaded);
        assert_eq!(text(&next), whole(&next));
        // And back: entries the cache dropped are written again.
        assert_eq!(text(&state), whole(&state));
    }
}
