//! `connect.ts`: the plugin's wiring, run once every capability exists: its
//! tools and pointer handlers on the interaction hub, the markup bridge to
//! the selection plugin, and the annotation commit sink on the actions
//! plugin. Every registration is released with the document.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_plugin_actions::ACTIONS;
use epdf_plugin_interaction::{
    INTERACTION, InteractionHandler, RegisterHandlerOptions, RegisterToolOptions,
};
use epdf_plugin_selection::SELECTION;

use crate::controller::Core;
use crate::tools::definitions::ARMED_STAMP_TOOL_ID;
use crate::tools::handlers::create_handlers;
use crate::tools::registry::hub_tool;

impl Core {
    /// `connectAnnotation(ctx, annotation)`.
    pub(crate) fn connect(&self) -> Result<(), Error> {
        let interaction = self.cx.get(&INTERACTION)?;
        let selection = self.cx.try_get(&SELECTION)?;
        // Script effects that hide or show annotations are written through this plugin.
        if let Some(actions) = self.cx.try_get(&ACTIONS)? {
            let me = self.weak();
            let dispose = actions.register_annot_commit_sink(Rc::new(move |entries| {
                let core = me.upgrade().ok_or_else(|| Error::new("[annotation] closed"))?;
                Ok(core.commit_script_effects(entries))
            }));
            self.cx.cleanup_fn(move || dispose());
        }
        // Every resolved tool; markup and caret tools need a selection plugin;
        // a tool a sibling already registered is left alone.
        for tool in self.list_resolved_tools() {
            if tool.enables("text-select") && selection.is_none() {
                continue;
            }
            if interaction.has_tool(&tool.id) {
                continue;
            }
            let unregister =
                interaction.register_tool(hub_tool(&tool), RegisterToolOptions::default())?;
            self.cx.cleanup_fn(move || unregister.call());
        }
        let (place, edit, marquee, draw) = create_handlers(self, &interaction);
        draw.wire(self);
        let handlers: [Rc<dyn InteractionHandler>; 4] = [place, edit, marquee, draw];
        for handler in handlers {
            let unregister =
                interaction.register_handler(handler, RegisterHandlerOptions::default());
            self.cx.cleanup_fn(move || unregister.call());
        }
        let (me, i) = (self.weak(), interaction.clone());
        let off = interaction.on_tool_changed(move |_| {
            let Some(core) = me.upgrade() else { return Ok(()) };
            core.cancel();
            core.clear_ghost();
            core.prune_engaged_selection();
            let active = i.get_active_tool();
            if active.id != ARMED_STAMP_TOOL_ID && !active.enables("annotation-stamp") {
                core.stamps_disarm();
            }
            Ok(())
        });
        self.cx.listen(off);
        if let Some(selection) = selection {
            self.wire_markup(&selection, &interaction);
        }
        Ok(())
    }
}
