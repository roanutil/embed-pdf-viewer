//! `packages/plugin/commands` in Rust: the command registry, the single
//! vocabulary of verbs. Definitions are app content; a command's label,
//! accent, enablement, active state and visibility derive at call time from
//! i18n, the shell and the definition's own derivations.
//!
//! The file split follows the TypeScript package: [`types`], [`reducer`],
//! [`capability`], and the wiring in [`commands_plugin`].

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::{AnyPlugin, Error, define};

pub mod capability;
pub mod reducer;
pub mod types;

#[cfg(feature = "harness")]
pub mod harness;

pub use capability::{CommandRegistry, CommandsCapability, RegisteredCommand, register_command};
pub use reducer::{commands_reducer, initial_commands_state};
pub use types::{
    AccentDerivation, COMMANDS, CommandCtx, CommandDef, CommandsAction, CommandsConfig,
    CommandsState, Derivation, IconAccent, MenuTarget, Panel, ResolvedCommand, RunFn, Shortcut,
    resolved_commands_equal,
};

/// `commandsPlugin(config)`: workspace-scoped, one vocabulary for the whole
/// workspace, with resolution and execution bound to a target document per
/// call. Definitions live in this closure, never in the store. A duplicate
/// id or an unparsable shortcut in `config.commands` is the error the
/// TypeScript throws from the factory.
pub fn commands_plugin(config: CommandsConfig) -> Result<AnyPlugin, Error> {
    let mut registry = CommandRegistry::new();
    for def in config.commands {
        register_command(&mut registry, def)?;
    }
    let registry = Rc::new(RefCell::new(registry));
    let disabled_categories = config.disabled_categories;

    Ok(define::<CommandsState, CommandsAction>("commands")
        .initial_state(move || CommandsState { disabled_categories: disabled_categories.clone() })
        .reduce(commands_reducer)
        .capability(&COMMANDS, move |ctx| Ok(CommandsCapability::new(ctx, registry.clone())))
        .build())
}
