//! `model.ts`: the link slice, loaded links per page, keyed by page object
//! number.

use std::collections::BTreeMap;

use epdf_annotation_core::{PageObjectNumber, PageRef};
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};

use crate::contract::Link;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LinkState {
    #[serde(deserialize_with = "pages_by_key", serialize_with = "pages_in_source_order")]
    pub pages: BTreeMap<PageObjectNumber, Vec<Link>>,
}

/// A link as `source.ts` writes it: `{id, bounds, target, activate?,
/// hoverEvents?, ref, attached}`. `Link` is the annotation plugin's
/// `LinkNavItem`, whose own producer (`read/render.ts`) writes `attached`
/// third, so the link slice orders its items here rather than in the type.
pub struct InSourceOrder<'a>(pub &'a Link);

impl Serialize for InSourceOrder<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let link = self.0;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("id", &link.id)?;
        map.serialize_entry("bounds", &link.bounds)?;
        map.serialize_entry("target", &link.target)?;
        if let Some(activate) = &link.activate {
            map.serialize_entry("activate", activate)?;
        }
        if let Some(hover) = &link.hover_events {
            map.serialize_entry("hoverEvents", hover)?;
        }
        if let Some(ref_) = &link.ref_ {
            map.serialize_entry("ref", ref_)?;
        }
        map.serialize_entry("attached", &link.attached)?;
        map.end()
    }
}

/// `items` in [`InSourceOrder`].
pub fn items_in_source_order<S: Serializer>(
    items: &[Link],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(items.iter().map(InSourceOrder))
}

fn pages_in_source_order<S: Serializer>(
    pages: &BTreeMap<PageObjectNumber, Vec<Link>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    struct Items<'a>(&'a [Link]);
    impl Serialize for Items<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            items_in_source_order(self.0, serializer)
        }
    }
    serializer.collect_map(pages.iter().map(|(pon, items)| (pon, Items(items))))
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
    SetPage {
        page: PageRef,
        #[serde(serialize_with = "items_in_source_order")]
        items: Vec<Link>,
    },
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
