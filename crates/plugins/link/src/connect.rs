//! `connect.ts`: the link plugin's wiring to its siblings, run from the
//! controller's `connect`.

use std::rc::Rc;

use epdf_kernel::{ControllerContext, Error};
use epdf_plugin_annotation::{ANNOTATION, Behavior};
use epdf_plugin_interaction::{INTERACTION, InteractionCapability};
use serde_json::{Map, Value};

/// The behavior `connect` registers with the annotation plugin: link
/// annotations stay navigation-owned while the active tool enables
/// `link-nav`.
pub struct LinkNavBehavior {
    interaction: InteractionCapability,
}

impl Behavior for LinkNavBehavior {
    fn id(&self) -> String {
        "link-nav".into()
    }

    fn matches(&self, annotation: &Map<String, Value>) -> bool {
        annotation.get("subtype").and_then(Value::as_str) == Some("link")
    }

    fn engaged(&self, _annotation: &Map<String, Value>) -> bool {
        self.interaction.active_tool_enables("link-nav")
    }
}

/// `connectLink(ctx)`: links are annotations, so while a navigation tool
/// is active the link layer owns their input (its anchors take the clicks)
/// and the annotation plane stands down for them. A link still shows as
/// the file draws it.
pub(crate) fn connect_link(cx: &ControllerContext) -> Result<(), Error> {
    let Some(annotation) = cx.try_get(&ANNOTATION)? else { return Ok(()) };
    let interaction = cx.get(&INTERACTION)?;
    cx.listen(annotation.register_behavior(Rc::new(LinkNavBehavior { interaction })));
    Ok(())
}
