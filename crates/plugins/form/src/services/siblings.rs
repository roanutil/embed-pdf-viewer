//! `services/siblings.ts`: the optional sibling planes, looked up each time
//! they're used, not once at construction. A fill-only setup may have none;
//! every area guards on `None`.

use epdf_plugin_actions::{ACTIONS, ActionsCapability};
use epdf_plugin_annotation::{ANNOTATION, AnnotationCapability};
use epdf_plugin_interaction::{INTERACTION, InteractionCapability};
use epdf_plugin_render::{RENDER, RenderCapability};

impl crate::controller::Core {
    /// The annotation plugin: design mode's selection, tools, ghosts and
    /// placement previews.
    pub(crate) fn annotation(&self) -> Option<AnnotationCapability> {
        self.cx.try_get(&ANNOTATION).ok().flatten()
    }

    /// The script realm owner and the one surface port for script results.
    pub(crate) fn actions(&self) -> Option<ActionsCapability> {
        self.cx.try_get(&ACTIONS).ok().flatten()
    }

    /// The page renderer: the widgets' pictures.
    pub(crate) fn render(&self) -> Option<RenderCapability> {
        self.cx.try_get(&RENDER).ok().flatten()
    }

    /// The interaction hub, which the plugin requires.
    pub(crate) fn interaction(&self) -> Option<InteractionCapability> {
        self.cx.try_get(&INTERACTION).ok().flatten()
    }
}
