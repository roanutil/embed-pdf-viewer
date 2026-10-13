//! `packages/plugin/commands` in Rust: the command registry, the single
//! vocabulary of verbs. Definitions are app content; a command's label,
//! accent, enablement, active state and visibility derive at call time from
//! i18n, the shell and the definition's own derivations.
//!
//! The file split follows the TypeScript package: [`contract`], [`model`],
//! [`registry`], [`controller`], and the wiring in [`commands_plugin`].

use std::rc::Rc;

use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_i18n::I18N;
use epdf_plugin_shell::SHELL;

pub mod contract;
pub mod controller;
pub mod model;
pub mod registry;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

#[cfg(feature = "harness")]
pub mod harness;

pub use contract::{
    AccentDerivation, COMMANDS, CommandCtx, CommandDef, CommandExecutedEvent,
    CommandExecutionFailedEvent, CommandsConfig, Derivation, ExecuteResult, IconAccent, MenuTarget,
    Panel, RegisterCommandOptions, RejectReason, ResolvedCommand, RunFn, Shortcut, ShortcutBinding,
    resolved_commands_equal,
};
pub use controller::{CommandsCapability, WeakCommandsCapability, create_commands_controller};
pub use model::{CommandsAction, CommandsState, commands_reducer, initial_commands_state};
pub use registry::{CommandRegistry, RegisteredCommand, register_command};

/// `commandsPlugin(config)`: workspace-scoped, one vocabulary for the whole
/// workspace, with resolution and execution bound to a target document per
/// call. Each controller registers the configured definitions in its own
/// registry, so a bad definition fails the kernel, not this call. i18n and
/// the shell are optional: without them labels stay keys and surface targets
/// route nowhere.
pub fn commands_plugin(config: CommandsConfig) -> AnyPlugin {
    let config = Rc::new(config);
    let disabled = config.disabled_categories.clone();
    define::<CommandsState, CommandsAction>("commands")
        .optional(I18N.any())
        .optional(SHELL.any())
        .initial_state(move || initial_commands_state(&disabled))
        .reduce(commands_reducer)
        .create(&COMMANDS, move |cx| Ok((create_commands_controller(cx, &config)?, None)))
        .build()
}
