//! Every plugin crate's conformance adapter, so a plugin spec can name any
//! factory and a capability call can reach any plugin by name.

use std::rc::Rc;

use plugin_conformance::{HarnessPlugin, Registry};

/// `documents` and the probe (`Registry::new`), then one adapter per plugin
/// crate this crate links. Each adapter only answers for its own capability
/// type, so the order does not matter.
pub fn adapters() -> Vec<Rc<dyn HarnessPlugin>> {
    Registry::new()
        .with(epdf_plugin_actions::harness::Actions)
        .with(epdf_plugin_annotation::harness::Annotation)
        .with(epdf_plugin_attachment::harness::Attachment)
        .with(epdf_plugin_commands::harness::Commands)
        .with(epdf_plugin_form::harness::Form)
        .with(epdf_plugin_history::harness::History)
        .with(epdf_plugin_i18n::harness::I18n)
        .with(epdf_plugin_interaction::harness::Interaction)
        .with(epdf_plugin_link::harness::LinkAdapter)
        .with(epdf_plugin_measurement::harness::MeasurementAdapter)
        .with(epdf_plugin_metadata::harness::Metadata)
        .with(epdf_plugin_outline::harness::Outline)
        .with(epdf_plugin_page_edit::harness::PageEdit)
        .with(epdf_plugin_redaction::harness::Redaction)
        .with(epdf_plugin_render::harness::Render)
        .with(epdf_plugin_search::harness::Search)
        .with(epdf_plugin_selection::harness::Selection)
        .with(epdf_plugin_shell::harness::Shell)
        .with(epdf_plugin_signature::harness::Signature)
        .with(epdf_plugin_stage::harness::Stage)
        .with(epdf_plugin_stamp::harness::Stamp)
        .with(epdf_plugin_view_manager::harness::ViewManager)
        .adapters
}
