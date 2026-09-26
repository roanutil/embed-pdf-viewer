//! `model.ts`: the link slice, loaded links per page, keyed by page object
//! number.

use std::collections::BTreeMap;

use epdf_annotation_core::{PageObjectNumber, PageRef};
use serde::{Deserialize, Serialize};

use crate::contract::Link;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LinkState {
    #[serde(deserialize_with = "pages_by_key")]
    pub pages: BTreeMap<PageObjectNumber, Vec<Link>>,
}

/// Object keys are strings on the wire; the cache only ever holds page
/// object numbers.
fn pages_by_key<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<PageObjectNumber, Vec<Link>>, D::Error> {
    BTreeMap::<String, Vec<Link>>::deserialize(deserializer)?
        .into_iter()
        .map(|(key, items)| {
            key.parse().map(|pon| (pon, items)).map_err(|_| {
                serde::de::Error::custom(format!("page key {key:?} is not a page object number"))
            })
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LinkAction {
    #[serde(rename = "setPage")]
    SetPage { page: PageRef, items: Vec<Link> },
    #[serde(rename = "dropPage")]
    DropPage { page: PageRef },
}

pub fn initial_link_state() -> LinkState {
    LinkState::default()
}

/// Pure. `None` is the TypeScript's `return state`.
pub fn link_reducer(state: &LinkState, action: &LinkAction) -> Option<LinkState> {
    match action {
        LinkAction::SetPage { page, items } => {
            let mut pages = state.pages.clone();
            pages.insert(page.page_object_number, items.clone());
            Some(LinkState { pages })
        }
        LinkAction::DropPage { page } => {
            let pon = page.page_object_number;
            if !state.pages.contains_key(&pon) {
                return None;
            }
            let mut pages = state.pages.clone();
            pages.remove(&pon);
            Some(LinkState { pages })
        }
    }
}
