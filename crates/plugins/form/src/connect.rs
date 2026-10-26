//! `connect.ts`: the form plugin's wiring to its siblings, run once every
//! capability exists: its tools and place handler on the interaction hub,
//! its widgets lent to the annotation plugin for design, and the reset
//! executor, commit sink, form reader and submit resolver on the actions
//! plugin. Every registration is released with the document.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use epdf_plugin_actions::{
    ActionExecutor, ActionExecutorResult, ActionOrigin, FormCommitSink, FormReader, SubmitResolver,
};
use epdf_plugin_interaction::{INTERACTION, RegisterHandlerOptions, RegisterToolOptions, Tool};
use serde_json::{Value, json};

use crate::contract::FormCommitStatus;
use crate::controller::Core;
use crate::tools::definitions::{FORM_TOOLS, PLACE_TAGS};
use crate::tools::handlers::create_place_handler;
use crate::value::{get, str_of};
use crate::write::lending::FormLender;

impl Core {
    /// `connectForm(ctx, form, lender)`.
    pub(crate) fn connect(&self) -> Result<(), Error> {
        let interaction = self.cx.get(&INTERACTION)?;
        let annotation = self.annotation();

        // Fill-only mode: forms are fillable and annotations are not
        // editable. 'link-nav' keeps links navigable while filling.
        let off = interaction.register_tool(
            Tool::new("form-fill", "default", ["form-fill", "link-nav"]),
            RegisterToolOptions::default(),
        )?;
        self.cx.cleanup_fn(move || off.call());
        // Design mode's resting tool: without 'form-fill' the widget
        // behavior disengages, and widgets select, move and resize like any
        // annotation.
        let off = interaction.register_tool(
            Tool::new("form-edit", "default", ["annotation-edit", "annotation-marquee"]),
            RegisterToolOptions::default(),
        )?;
        self.cx.cleanup_fn(move || off.call());

        // The field palette: with the annotation plugin the tools join its
        // registry; without it they are plain hub tools. Either way the form
        // place handler commits them.
        for tool in FORM_TOOLS.iter() {
            match &annotation {
                Some(annotation) => {
                    let off = annotation.tools_register(json!({
                        "id": tool.id,
                        "subtype": tool.visual_kind,
                        "cursor": tool.cursor,
                        "enables": PLACE_TAGS,
                        "clickCreate": tool.click_create_json(),
                        "defaults": tool.defaults,
                        // Click-first: the field a click places follows the pointer.
                        "ghost": true,
                    }))?;
                    self.cx.cleanup_fn(move || off.call());
                }
                None => {
                    let off = interaction.register_tool(
                        Tool::new(tool.id, tool.cursor, PLACE_TAGS),
                        RegisterToolOptions::default(),
                    )?;
                    self.cx.cleanup_fn(move || off.call());
                }
            }
        }
        let handler = create_place_handler(self, interaction.clone(), annotation.clone());
        let off = interaction.register_handler(handler, RegisterHandlerOptions::default());
        self.cx.cleanup_fn(move || off.call());

        // Design mode: the widgets are lent to the annotation plugin.
        if let Some(annotation) = &annotation
            && self.allows("doc.forms.modify")
        {
            let off = annotation.lend(FormLender::new(self));
            self.cx.cleanup_fn(move || off.call());
        }

        let Some(actions) = self.actions() else { return Ok(()) };
        // Script form effects are written through this plugin, which owns
        // the fields, and scripts read the form as the user sees it.
        let me = self.weak();
        let sink: FormCommitSink = Rc::new(move |effects| match me.upgrade() {
            Some(core) => Ok(core.commit_script_form_effects(effects)),
            None => Err(Error::new("[form] the document closed")),
        });
        let off = actions.register_form_commit_sink(sink);
        self.cx.cleanup_fn(move || off());
        let me = self.weak();
        let reader: FormReader = Rc::new(move || me.upgrade().and_then(|core| core.get_snapshot()));
        let off = actions.register_form_reader(reader);
        self.cx.cleanup_fn(move || off());
        let me = self.weak();
        let resolver: SubmitResolver =
            Rc::new(move |intent, action_context, diagnose| match me.upgrade() {
                Some(core) => Ok(core.resolve_submit_dataset(
                    intent.clone(),
                    action_context.clone(),
                    diagnose,
                )),
                None => Err(Error::new("[form] the document closed")),
            });
        let off = actions.register_submit_resolver(resolver);
        self.cx.cleanup_fn(move || off());
        // The executor passes its action context through, so a ResetForm run
        // by a page or document trigger reports its recalculation with that
        // origin.
        let me = self.weak();
        let executor: ActionExecutor = Rc::new(move |node, action_context| {
            let Some(core) = me.upgrade() else {
                return Err(Error::new("[form] the document closed"));
            };
            if str_of(node, "type") != Some("reset-form") {
                return Ok(Promise::resolved(ActionExecutorResult::Inert {
                    reason: "not a reset-form node".into(),
                }));
            }
            let targets = get(node, "fields").and_then(Value::as_array).cloned();
            // `node.exclude`, passed as it is.
            let exclude = get(node, "exclude").cloned().unwrap_or_else(epdf_kernel::undefined);
            let origin: ActionOrigin = action_context.origin;
            let running = core.reset_form_action(targets, exclude, Some(origin));
            Ok(Promise::spawn(async move {
                let result = running.await?;
                Ok(match result.status {
                    FormCommitStatus::Failed => ActionExecutorResult::Failed {
                        error: result
                            .error
                            .map(|e| e.message)
                            .unwrap_or_else(|| "reset failed".into()),
                    },
                    FormCommitStatus::Unchanged => ActionExecutorResult::Inert {
                        reason: result
                            .diagnostics
                            .first()
                            .map(|d| d.message.clone())
                            .unwrap_or_else(|| "reset-form resolved no fields".into()),
                    },
                    _ => ActionExecutorResult::Executed,
                })
            }))
        });
        let off = actions.register_executor("reset-form", executor);
        self.cx.cleanup_fn(move || off());
        Ok(())
    }
}
