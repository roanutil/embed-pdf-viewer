//! `helpers.ts`: the bundle handed to `chrome: (base, h) => …` transforms.
//! Its shape is public API, so the member names and their order are pinned
//! in [`CHROME_HELPER_KEYS`].

use epdf_kernel::Error;

use crate::schema::{
    BarChild, BarGroup, BarItem, ChromeSchema, CustomItem, CustomOpts, GroupOptsOrItems, ItemOpts,
    custom, define_chrome, group, item,
};
use crate::transforms::{AddItemSpec, add_item, remove_items, replace_item};

#[derive(Clone, Copy)]
pub struct ChromeHelpers {
    pub add_item: fn(&ChromeSchema, &AddItemSpec) -> Result<ChromeSchema, Error>,
    pub remove_items: fn(&ChromeSchema, &[String]) -> ChromeSchema,
    pub replace_item: fn(&ChromeSchema, &str, &BarChild) -> ChromeSchema,
    pub item: fn(&str, Option<ItemOpts>) -> BarItem,
    pub group: fn(&str, GroupOptsOrItems, Option<Vec<BarChild>>) -> BarGroup,
    pub custom: fn(&str, CustomOpts) -> CustomItem,
    pub define_chrome: fn(ChromeSchema) -> Result<ChromeSchema, Error>,
}

/// `Object.keys(chromeHelpers)`.
pub const CHROME_HELPER_KEYS: [&str; 7] =
    ["addItem", "removeItems", "replaceItem", "item", "group", "custom", "defineChrome"];

pub const CHROME_HELPERS: ChromeHelpers =
    ChromeHelpers { add_item, remove_items, replace_item, item, group, custom, define_chrome };
