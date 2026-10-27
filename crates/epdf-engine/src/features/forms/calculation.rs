//! The form's calculation order (`/AcroForm /CO`): the fields whose
//! calculate scripts run, in the order they run, by object number.
//!
//! Ports `services/src/features/forms/internal/calculationOrder.ts`. A
//! field's calculate script keeps it in the order; the order is a list like
//! a page's annotations, moved by neighbour.

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineErrorCode};

/// The calculation order before a write and as the write left it
/// (`OrderChange`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderChange {
    pub before: Vec<u32>,
    pub after: Vec<u32>,
}

/// The order a loaded form model reads (`readCalculationOrder`). A slot that
/// names no field is left out, and a field named twice is listed once.
pub fn read_calculation_order(model: &pdfium::FormModel<'_>) -> Vec<u32> {
    let mut order = Vec::new();
    for slot in 0..model.calculation_order_count() {
        let Ok(index) = u32::try_from(model.calculation_order_field(slot)) else {
            continue;
        };
        let Some(field) = model.field(index) else { continue };
        let object_number = field.object_number();
        if object_number > 0 && !order.contains(&object_number) {
            order.push(object_number);
        }
    }
    order
}

/// Write the order; an empty one removes `/CO` (`writeCalculationOrder`).
pub fn write_calculation_order(
    document: &pdfium::Document,
    order: &[u32],
) -> Result<(), EngineError> {
    if !document.set_calculation_order(order) {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "the calculation order could not be written",
        ));
    }
    Ok(())
}

/// Whether two orders list the same fields in the same order (`sameOrder`).
pub fn same_order(a: &[u32], b: &[u32]) -> bool {
    a == b
}

/// The field right before `object_number` in `order`: `Some(None)` at the
/// start, `None` when it isn't there (`previous`).
fn previous(order: &[u32], object_number: u32) -> Option<Option<u32>> {
    let at = order.iter().position(|n| *n == object_number)?;
    Some(at.checked_sub(1).map(|before| order[before]))
}

/// `orderBack`: the order an undo leaves: `now`, with what a change did
/// (`before` to `after`) taken back where nobody changed it since.
///
/// - A field the change added leaves.
/// - A field it took out comes back, when it is still a field.
/// - A field it moved goes back, when the field before it is still the one
///   the change left there.
///
/// A field that comes or goes back lands right after its nearest earlier
/// neighbour of `before` that is in the order, or at the start. Fields go
/// back lowest first, so a neighbour that moved too is back before them.
pub fn order_back(now: &[u32], change: &OrderChange, is_field: impl Fn(u32) -> bool) -> Vec<u32> {
    let OrderChange { before, after } = change;
    let added: Vec<u32> = after.iter().copied().filter(|n| !before.contains(n)).collect();
    let mut order: Vec<u32> = now.iter().copied().filter(|n| !added.contains(n)).collect();
    for (at, object_number) in before.iter().copied().enumerate() {
        let taken_out = !after.contains(&object_number);
        if taken_out {
            if order.contains(&object_number) || !is_field(object_number) {
                continue;
            }
        } else {
            let moved = previous(before, object_number) != previous(after, object_number);
            if !moved || previous(now, object_number) != previous(after, object_number) {
                continue;
            }
            order.retain(|other| *other != object_number);
        }
        let neighbour = before[..at].iter().rev().find(|other| order.contains(other));
        let index = match neighbour {
            None => 0,
            Some(neighbour) => order.iter().position(|n| n == neighbour).map_or(0, |i| i + 1),
        };
        order.insert(index, object_number);
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_undo_takes_back_what_the_change_did_where_nobody_changed_it_since() {
        // Added: leaves.
        let added = OrderChange { before: vec![1, 2], after: vec![1, 2, 3] };
        assert_eq!(order_back(&[1, 2, 3], &added, |_| true), vec![1, 2]);
        // Taken out: comes back after its old neighbour, while it is a field.
        let taken = OrderChange { before: vec![1, 2, 3], after: vec![1, 3] };
        assert_eq!(order_back(&[1, 3], &taken, |_| true), vec![1, 2, 3]);
        assert_eq!(order_back(&[1, 3], &taken, |n| n != 2), vec![1, 3]);
        // Moved: goes back while it follows what the change put before it.
        let moved = OrderChange { before: vec![1, 2, 3], after: vec![2, 3, 1] };
        assert_eq!(order_back(&[2, 3, 1], &moved, |_| true), vec![1, 2, 3]);
        assert_eq!(order_back(&[1, 2, 3], &moved, |_| true), vec![1, 2, 3]);
    }
}
