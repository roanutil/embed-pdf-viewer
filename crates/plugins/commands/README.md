# epdf-plugin-commands

`packages/plugin/commands` in Rust. All 52 cases in `vectors/plugins/commands/functions.json` (the
reducer, `resolvedCommandsEqual`, `registerCommand`, what `commandsPlugin(config)` builds) and all
19 scenarios in `vectors/plugins/commands/plugin.json` replay identically
(`cargo test -p epdf-plugin-commands`). The scenarios run the plugin alone and next to
`epdf-plugin-i18n` and `epdf-plugin-shell`, so the test registry carries all three adapters.

The plugin is workspace-scoped and its capability exists from `Kernel::new`. Each `resolve` and
`execute` binds a `CommandCtx` to the document it names, else the active one. Labels come from
`I18N` when it resolves, and `active` comes from the target document's `SHELL` unless the definition
overrides it.

## Command functions in the vectors

A definition carries functions (`run`, `enabled`, `active`, `visible`, `iconAccent`), and a vector
writes each as a `$fn` spec. The runner decodes the ones in a `register` step's arguments. The
factory decodes the ones in `config.commands`, and those take one more member, `uses`: things to do
with the `CommandCtx` before answering (`get` or `tryGet` a token and optionally call a method on
it, or read a member of `core()`). Their results are logged as the callback `<name>:uses`. That is
how `command-ctx` pins every refusal `get` can return (no active document, no provider, a loading
document, an unknown document, a throwing capability constructor) and that `tryGet` turns each one
into `null`. `harness.rs` is the Rust half of the same decoder.

What the vectors pin about those functions:

- `resolve` calls `active`, then `iconAccent`, then `enabled`, then `visible`, each with a fresh
  context. It calls them even for a command in a disabled category, whose `enabled` and `visible`
  are then false anyway (`categories`).
- A throw falls back: `enabled` and `visible` to true, `active` to false, the accent to none
  (`throwing-derivations-fall-back`, `icon-accent`).
- `execute` resolves first, refuses a disabled or hidden command, and then calls `run` with a
  second fresh context. A `run` that throws throws out of `execute`. One that rejects or stays
  pending is never awaited (`execute-guards-and-run`).
- A command with `run` never gets the surface routing, whatever the doc comment on `run` in
  `types.ts` says ("Runs before the default target routing when both are present"). The code
  returns after `run`, and the port follows the code.
- An explicit empty document id is kept as the context's `documentId`, but because it is falsy the
  TypeScript resolves through `ctx.get` against the active document. `CommandCtx::get_any` does
  the same.

## JavaScript semantics

The registry is a `Map`, so `CommandRegistry` keeps its iteration order: by insertion, and an id
unregistered and registered again moves to the end. `search` walks the registry live, as `for...of`
over a `Map` does, and a derivation can change the registry mid-walk. An entry deleted ahead of the
walk is skipped and one added during it is visited (`re-entrant-registry`); deleting one behind the
walk skips nothing (`unregister-behind-a-search`). Deletions leave a hole during a walk and are
compacted when it ends. The cost is a walk counter and an index rebuild on every delete, which is
linear in the number of commands.

`menu` and `modal` are truthiness checks in the TypeScript, so an empty id is no target, while an
empty `panel` string is still a panel (`shell-derived-active`, `execute-routing`). `search` trims
with the ECMAScript whitespace set (`epdf_ui_core::js::trim`) and lowercases the query and the
label but not the id, so `Zoom:O` finds nothing in an id `Zoom:Out`.

After `destroy`, a capability reference someone kept still reads and changes the registry and
matches strokes. Everything that reads the slice throws, because `getState()` is `undefined` there:
`resolve`, `search`, `execute`, `disabledCategories` and `isCategoryDisabled` return
`Err(TypeError: Cannot read properties of undefined (reading 'disabledCategories'))`, which is why
they return `Result` (`after-destroy`).

## Coverage and what is not ported

The generators reach every statement and branch of the package: `capability.ts` 191/191 and 91/91,
`commands.plugin.ts` 29/29 and 8/8, `reducer.ts` 22/22 and 11/11, `types.ts` 168/168 and 21/21.

Answers outside the TypeScript types are not ported. A derivation typed `boolean` that returns
something else would reach the resolved command through `&&` unchanged when falsy (`enabled: 0`),
and Rust's derivations return `bool`; the harness reads an answer by JavaScript truthiness. An
accent object with members beyond `primary` and `secondary` loses them here. A derivation that
returns a promise is truthy in the TypeScript, and an `active` promise would even land in the
resolved command; Rust's derivations are synchronous.

`resolvedCommandsEqual`'s `a === b` shortcut has no Rust counterpart. Every member it compares is
a string, a boolean or a list of strings, so an object always equals itself by value, and
`resolvedCommandsEqualSelf` pins that.

## Mutation testing

`cargo mutants -p epdf-plugin-commands --test-package epdf-plugin-commands` catches 107 of 113
viable mutants and times out on 4, each of which makes the registry walk loop forever. Two survive,
and both are equivalent: `initial_commands_state()` replaced with `Default::default()` builds the
same value, and `CommandDef`'s `Debug` impl only formats. The compaction mutants are killed by the
unit test in `capability.rs`, since reclaiming a slot is memory, not behaviour a vector can see.
