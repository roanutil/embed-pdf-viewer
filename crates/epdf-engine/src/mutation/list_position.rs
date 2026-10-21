//! Where rows go in their list (`core/src/mutation/ListPosition.ts`).
//!
//! A position names a neighbour, never a number, so a change keeps its
//! meaning when other rows come and go before it applies, and a caller only
//! names rows it can see. Each list is in its own order: a page's
//! annotations, and its widgets, each in paint order, later over earlier.

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineErrorCode};

/// Right before or after a neighbour, or at the start or the end
/// (`ListPosition`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ListPosition<Ref> {
    Before { before: Ref },
    After { after: Ref },
    End(End),
}

/// `'start'` or `'end'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum End {
    Start,
    End,
}

/// Where annotations go among their page's annotations, or widgets among
/// its widgets (`AnnotationPosition`).
pub type AnnotationPosition = ListPosition<crate::identity::AnnotationRef>;

impl<Ref> ListPosition<Ref> {
    pub const START: Self = Self::End(End::Start);
    pub const END: Self = Self::End(End::End);

    /// The neighbour a position names, or `None` for the start and the end
    /// (`anchorOf`).
    pub fn anchor(&self) -> Option<&Ref> {
        match self {
            Self::Before { before } => Some(before),
            Self::After { after } => Some(after),
            Self::End(_) => None,
        }
    }
}

/// Where `position` falls in `order`: the index the first row placed there
/// gets (`positionIndex`). The neighbour must be in `order` (`NotFound`
/// otherwise).
pub fn position_index<Ref, Key: PartialEq + std::fmt::Display>(
    order: &[Ref],
    position: &ListPosition<Ref>,
    key_of: impl Fn(&Ref) -> Key,
) -> Result<usize, EngineError> {
    let (anchor, after) = match position {
        ListPosition::End(End::Start) => return Ok(0),
        ListPosition::End(End::End) => return Ok(order.len()),
        ListPosition::Before { before } => (before, false),
        ListPosition::After { after } => (after, true),
    };
    let key = key_of(anchor);
    let at = order.iter().position(|row| key_of(row) == key).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("no neighbour {key} in the list"),
        )
        .with_details(serde_json::json!({ "field": "position" }))
    })?;
    Ok(if after { at + 1 } else { at })
}

/// A list's order once `moved` go to `position`, together and in the order
/// given (`reorderedList`). Every moved row must be in `order`, once
/// (`InvalidArg` for a row named twice, `NotFound` for one not in it), and
/// the neighbour can't be one of them (`InvalidArg`).
pub fn reordered_list<Ref: Clone, Key: PartialEq + std::fmt::Display>(
    order: &[Ref],
    moved: &[Ref],
    position: &ListPosition<Ref>,
    key_of: impl Fn(&Ref) -> Key,
) -> Result<Vec<Ref>, EngineError> {
    let moved_keys: Vec<Key> = moved.iter().map(&key_of).collect();
    if moved_keys.iter().enumerate().any(|(i, key)| moved_keys[..i].contains(key)) {
        return Err(EngineError::with_message(EngineErrorCode::InvalidArg, "a row is named twice")
            .with_details(serde_json::json!({ "field": "refs" })));
    }
    let present: Vec<Key> = order.iter().map(&key_of).collect();
    if let Some(missing) = moved_keys.iter().find(|key| !present.contains(key)) {
        return Err(EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("no row {missing} in the list"),
        )
        .with_details(serde_json::json!({ "field": "refs" })));
    }
    if let Some(anchor) = position.anchor().map(&key_of)
        && moved_keys.contains(&anchor)
    {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("the neighbour {anchor} is one of the rows that move"),
        )
        .with_details(serde_json::json!({ "field": "position" })));
    }
    let staying: Vec<Ref> =
        order.iter().filter(|row| !moved_keys.contains(&key_of(row))).cloned().collect();
    let at = position_index(&staying, position, &key_of)?;
    let mut out = Vec::with_capacity(order.len());
    out.extend_from_slice(&staying[..at]);
    out.extend_from_slice(moved);
    out.extend_from_slice(&staying[at..]);
    Ok(out)
}

/// `list` with the items `in_part` picks put in the order `order` gives
/// them, by key, in the slots those items have: every other item keeps its
/// place (`reorderPart`). Items of the part `order` doesn't name follow the
/// named ones, in their order.
pub fn reorder_part<Item: Clone, Key: PartialEq>(
    list: &[Item],
    in_part: impl Fn(&Item) -> bool,
    order: &[Key],
    key_of: impl Fn(&Item) -> Key,
) -> Vec<Item> {
    let part: Vec<&Item> = list.iter().filter(|item| in_part(item)).collect();
    let mut placed = vec![false; part.len()];
    let mut next: Vec<&Item> = Vec::with_capacity(part.len());
    for key in order {
        if let Some(at) = part.iter().position(|item| key_of(item) == *key)
            && !placed[at]
        {
            placed[at] = true;
            next.push(part[at]);
        }
    }
    next.extend(part.iter().zip(&placed).filter(|(_, placed)| !**placed).map(|(item, _)| *item));
    let mut slot = 0;
    list.iter()
        .map(|item| {
            if in_part(item) {
                slot += 1;
                next[slot - 1].clone()
            } else {
                item.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! `core/test/mutation/listPosition.test.ts`.
    use super::*;

    fn key(item: &&str) -> String {
        (*item).to_owned()
    }

    fn before(row: &'static str) -> ListPosition<&'static str> {
        ListPosition::Before { before: row }
    }

    fn after(row: &'static str) -> ListPosition<&'static str> {
        ListPosition::After { after: row }
    }

    #[test]
    fn a_neighbour_stands_for_its_own_index_or_the_next_start_and_end_for_the_ends() {
        let order = ["a", "b", "c"];
        assert_eq!(position_index(&order, &ListPosition::START, key).unwrap(), 0);
        assert_eq!(position_index(&order, &ListPosition::END, key).unwrap(), 3);
        assert_eq!(position_index(&order, &before("b"), key).unwrap(), 1);
        assert_eq!(position_index(&order, &after("b"), key).unwrap(), 2);
    }

    #[test]
    fn position_index_refuses_a_neighbour_that_is_not_in_the_list() {
        let refused = position_index(&["a"], &after("z"), key).unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::NotFound);
    }

    #[test]
    fn the_rows_go_together_in_the_order_given_next_to_the_neighbour() {
        let order = ["a", "b", "c", "d"];
        assert_eq!(reordered_list(&order, &["a"], &after("c"), key).unwrap(), ["b", "c", "a", "d"]);
        assert_eq!(
            reordered_list(&order, &["d", "a"], &before("c"), key).unwrap(),
            ["b", "d", "a", "c"]
        );
        assert_eq!(
            reordered_list(&order, &["c", "a"], &ListPosition::START, key).unwrap(),
            ["c", "a", "b", "d"]
        );
        assert_eq!(
            reordered_list(&order, &["a"], &ListPosition::END, key).unwrap(),
            ["b", "c", "d", "a"]
        );
    }

    #[test]
    fn a_row_named_twice_or_a_neighbour_that_moves_is_invalid_and_a_row_not_in_the_list_not_found()
    {
        let order = ["a", "b", "c", "d"];
        let code = |result: Result<Vec<&str>, EngineError>| result.unwrap_err().code;
        assert_eq!(
            code(reordered_list(&order, &["a", "a"], &ListPosition::END, key)),
            EngineErrorCode::InvalidArg
        );
        assert_eq!(
            code(reordered_list(&order, &["a", "b"], &after("b"), key)),
            EngineErrorCode::InvalidArg
        );
        assert_eq!(
            code(reordered_list(&order, &["z"], &ListPosition::END, key)),
            EngineErrorCode::NotFound
        );
    }

    #[test]
    fn one_part_takes_the_order_given_in_its_own_slots_the_rest_keep_their_places() {
        // Two pages' rows, interleaved: page 1's are the upper-case ones.
        let list = ["A", "x", "B", "y", "C"];
        let on_page = |item: &&str| *item == item.to_uppercase();
        assert_eq!(
            reorder_part(&list, on_page, &["C", "A", "B"], |i| *i),
            ["C", "x", "A", "y", "B"]
        );
    }

    #[test]
    fn what_the_order_leaves_out_follows_what_it_names() {
        let list = ["A", "B", "C"];
        assert_eq!(reorder_part(&list, |_| true, &["C", "Z"], |i| *i), ["C", "A", "B"]);
    }

    #[test]
    fn a_position_reads_as_the_wire_spells_it() {
        let wire: ListPosition<String> =
            serde_json::from_value(serde_json::json!("start")).unwrap();
        assert_eq!(wire, ListPosition::START);
        let wire: ListPosition<String> =
            serde_json::from_value(serde_json::json!({ "after": "b" })).unwrap();
        assert_eq!(wire, ListPosition::After { after: "b".into() });
        assert_eq!(serde_json::to_value(ListPosition::<String>::END).unwrap(), "end");
        assert!(
            serde_json::from_value::<ListPosition<String>>(serde_json::json!("middle")).is_err()
        );
    }
}
