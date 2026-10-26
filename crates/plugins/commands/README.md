# epdf-plugin-commands

`packages/plugin/commands` in Rust, re-ported in Task 13.29 to the current TypeScript
(`packages/plugin/commands/src/{controller,registry,services/availability,write/turning-off}.ts`).
`cargo test -p epdf-plugin-commands` replays `vectors/plugins/commands/functions.json` (54 cases) and
`vectors/plugins/commands/plugin.json` (25 scenarios), with no member-order allowance or outcome
exception, all 79 of them. `tests/behavior.rs` pins what the vectors can't print.

The plugin is workspace-scoped, has no state slice, and declares i18n and the shell as optional.
It sets `resolves_any_capability()`, so a command's `get` reaches tokens the plugin never declared
(`command-ctx`), and `in_scope`, so a capability the kernel resolves for a document is that
document's view: a call that leaves out the document reads that one (`command-ctx` step 12,
`the_kernel_hands_out_the_scoped_view`). `CommandsCapability::in_scope(document_id)` is the view.
The modules follow the TypeScript files: `contract.rs`, `registry.rs`, `controller.rs`,
`availability.rs` (`services/availability.ts`) and `turning_off.rs` (`write/turning-off.ts`).

## Settings

The commands, `disabledCategories`, `disabledCommands` and `notAllowed` are settings in the
kernel's settings store, so `update_settings`, `reset_settings` and `on_settings_changed` work as
for any plugin. A definition or a family holds functions, so `settings.rs` stores each as a handle,
`{"$command": n}` or `{"$commandFamily": n}`, into a table the registration owns. The table hands
out one handle per definition that is equal by `sameValue` (data members equal, functions the same
`Rc`), so a change that gives equal definitions changes nothing and reads back the definitions
already there, as the TypeScript merge keeps its array. Each list keeps its `Rc` while a change
leaves it equal (`a_settings_change_that_changes_nothing_fires_nothing`).

A configured list with an unparsable shortcut is not refused at construction. Every read that needs
the configured commands throws the parse error instead (`a-bad-config-shortcut-refuses-the-kernel`),
and a configured duplicate id resolves to the last definition in the first one's place
(`a-bad-config-refuses-the-kernel`). Commands registered while the app runs shadow the configured
ones, and still refuse a taken id with `conflict` unless `replace`.

## Reads, identity and failures

Every read the TypeScript memoizes (`resolveCommand`, `listCommands`, `listCommandIds`,
`listShortcuts`, the configured and defined commands) returns an `Rc` that stays the same until an
input changes. Each cache holds the inputs it compares: an `Rc` by `Rc::ptr_eq`, a string or a
boolean by value. Every derivation still runs on every resolve, as `memoByKey` runs its inputs.

A read that throws in the TypeScript returns `Result` here. Two things throw: the configured
shortcut above, and reading the active document on a destroyed kernel, which every call that
resolves for "the active document" does (`after-destroy`). A walk over the commands reads the ids it
started with, so a derivation that removes one ahead of it skips that one and one it registers is
not visited (`re-entrant-registry`, `a_walk_reads_the_ids_it_started_with`).

`execute` settles on the TypeScript's turns, measured with `tsx` (`execute_settles_on_the_typescripts_turns`).
A run that completes later keeps it pending; a signal rejects `operation-cancelled` without a
failure event.

## Kept Rust-only divergences

These are reached by no vector and stay divergent:

- U4 in part: a remover holds its registration weakly, but a definition closure that holds the
  strong capability still forms a cycle. `CommandsCapability::downgrade` gives closures a weak
  handle, which keeps the document scope.
- U7: removers are `Rc<dyn Fn()>`, called any number of times, where `on_*` return `Unsubscribe`.
- A family's `command` returns a whole `CommandDef`; its `id` and `shortcut` are ignored, as the
  TypeScript type omits them. The TypeScript spread would keep a returned `shortcut`.
- A family's commands are cached for the controller's life, where the TypeScript `WeakMap` lets
  them go with the family. The settings table likewise keeps every definition set for the
  registration's life (Task 14 retention note, as i18n's loader table).
- A settings value that doesn't decode (one no `update_settings` wrote) is reported to the kernel
  and read as empty, as i18n does.
- A derivation answer outside the TypeScript types (a non-boolean, a promise) and accent members
  beyond `primary` and `secondary` are not ported; the harness reads answers by JavaScript
  truthiness.

U1 (the `MenuTarget` type is ui-core's), U2 (`preserve_order`) and U8 stay as they were. U5, a
`register_commands` failure that leaves earlier registrations without a remover, is shared with the
TypeScript.

## Native exposure

`src/ffi.rs` and `tests/ffi.rs` are migrated by hand (fallible reads, `focus` on a panel,
`match_stroke` with its refusal) and not compiled until Task 13.33 rebuilds `epdf-ffi`. The
settings calls, `get_refusal`, per-command turning off, families, `requires` and `allowed`, an
`execute` signal and the scoped view are not exposed yet. `crates/epdf-web/src/exports.rs:27-33`
still documents a `ctx.core` marker on a command's context, which the harness no longer emits
(the current `CommandContext` has no `core`); that is a Task 13.33 carry.

## Coverage and mutation testing

The coverage figure and the mutation results this README recorded before Task 13.29 measured the
old reducer port. Neither was rerun for this port.
