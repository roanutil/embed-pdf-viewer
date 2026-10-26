//! `packages/plugin/commands` in Rust: the commands that toolbars, menus,
//! shortcuts and a command palette all show and run. A command's state is
//! read from live state: `resolve_command` asks the other capabilities when
//! it's called. Definitions hold functions, so they live in the settings
//! and the plugin's registry, never in state; the plugin has no slice.
//!
//! The file split follows the TypeScript package: [`contract`],
//! [`registry`], [`controller`], [`availability`]
//! (`services/availability.ts`), [`turning_off`] (`write/turning-off.ts`),
//! and the wiring in [`commands_plugin`]. [`settings`] keeps the settings in
//! the kernel's JSON settings store.

use std::rc::Rc;

use epdf_kernel::settings::SettingsDeclaration;
use epdf_kernel::{AnyPlugin, define, undefined};
use epdf_plugin_i18n::I18N;
use epdf_plugin_shell::SHELL;
use serde_json::Value;

pub mod availability;
pub mod contract;
pub mod controller;
pub mod registry;
pub mod settings;
pub mod turning_off;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

#[cfg(feature = "harness")]
pub mod harness;

pub use contract::{
    AccentDerivation, COMMANDS, CommandCtx, CommandDef, CommandExecutedEvent,
    CommandExecutionFailedEvent, CommandFamily, CommandId, CommandItem, CommandRefusal,
    CommandsConfig, CommandsSettings, CommandsSettingsChanged, Derivation, ExecuteOptions,
    ExecuteResult, FamilyCommandFn, IconAccent, MatchStrokeOptions, MenuTarget, NamesFn,
    NotAllowedMode, Panel, RefusalReason, RegisterCommandOptions, RejectReason, ResolvedCommand,
    RunFn, Shortcut, ShortcutBinding, ShortcutMatch, resolved_commands_equal, unregistered_command,
};
pub use controller::{
    CommandsCapability, CommandsContext, WeakCommandsCapability, create_commands_controller,
};
pub use registry::{
    CommandRegistry, ConfiguredCommands, RegisteredCommand, configured_commands, registered_command,
};
pub use settings::CommandTable;

/// The plugin and its registration's table of definitions.
pub(crate) fn plugin_parts(config: Option<CommandsConfig>) -> (AnyPlugin, Rc<CommandTable>) {
    let table = Rc::new(CommandTable::default());
    let registered = config.as_ref().map(|config| table.encode(config));
    let plugin = define::<Value, Value>("commands")
        // No `state`: the plugin has no slice of its own.
        .initial_state(undefined)
        // Labels come from i18n, and commands that open a panel, menu or
        // dialog go through shell; without them a label is the command's
        // own, and those commands open nothing.
        .optional(I18N.any())
        .optional(SHELL.any())
        // `resolvesAnyCapability: true` (command definitions come from the
        // app and may use any capability) and `inScope` wait on the kernel,
        // which has neither yet (Task 13.29, NEEDS_CONTEXT).
        .settings(SettingsDeclaration {
            defaults: settings::defaults_json(),
            registered,
            whole: Vec::new(),
        })
        .create(&COMMANDS, {
            let table = table.clone();
            move |cx| Ok((create_commands_controller(cx, table.clone())?, None))
        })
        .build();
    (plugin, table)
}

/// `commandsPlugin(config)`: one set of commands for the whole workspace,
/// each resolved and run for a document (the one named, else the document
/// in scope, else the active one). `config` is the settings the app
/// registers, over the defaults; `None` registers none. i18n and the shell
/// are optional.
pub fn commands_plugin(config: impl Into<Option<CommandsConfig>>) -> AnyPlugin {
    plugin_parts(config.into()).0
}
