# epdf-plugin-commands

`packages/plugin/commands` in Rust, at the v3 contract. All 52 cases in
`vectors/plugins/commands/functions.json` (the reducer, `resolvedCommandsEqual`, `registerCommand`,
what `commandsPlugin(config)` builds) and all 24 scenarios in `vectors/plugins/commands/plugin.json`
replay identically (`cargo test -p epdf-plugin-commands`). The scenarios run the plugin alone and
next to `epdf-plugin-i18n` and `epdf-plugin-shell`, so the test registry carries all three adapters.

The modules follow the TypeScript files: `contract.rs`, `model.rs`, `registry.rs` and
`controller.rs`. The plugin is workspace-scoped, declares i18n and the shell as optional, and its
controller exists from `Kernel::new`. Each controller registers `config.commands` into its own
registry, so a duplicate id or an unparsable shortcut fails the kernel's construction, not
`commands_plugin` (`a-bad-config-refuses-the-kernel`, `a-bad-config-shortcut-refuses-the-kernel`).
The api follows the v3 names: `registerCommand` and `registerCommands` return removers and take
`replace`, `getCommand`, `listCommands`, `listShortcuts`, `canExecute`, `onExecuted`,
`onExecutionFailed`, and the host lens `matchStroke` and `getMenuTarget`.

`execute` returns a `Promise<ExecuteResult>`. A missing, hidden or disabled command resolves
`rejected` with that reason. Otherwise `run` is called with a fresh `CommandCtx` (which carries
`args` when the caller gave some) and awaited; a throw or a rejection is mapped through
`toPluginError`, announced on `onExecutionFailed` as `PluginErrorInfo`, and rejected with
(`execute-events`, `execute-guards-and-run`). A failing listener goes to stderr, where the
TypeScript writes to `console.error`.

`resolveCommand` and `listCommands` build fresh values on every call. The TypeScript hands back
the previous object while it is equal by value, which only reference identity could observe.

## Command functions in the vectors

A definition carries functions (`run`, `enabled`, `active`, `visible`, `iconAccent`), and a vector
writes each as a `$fn` spec. The runner decodes the ones in a `register` step's arguments. The
factory decodes the ones in `config.commands`, and those take one more member, `uses`: things to do
with the `CommandCtx` before answering (`get` or `tryGet` a token and optionally call a method on
it, read a member of `core()`, keep a returned remover with `remember` and call it later with
`forget`). Their results are logged as the callback `<name>:uses`. A use that gets a promise back
(a run that starts another `execute`) records `{returnedPromise: true}`, because a promise has no
canonical form in callback arguments. `harness.rs` is the Rust half of the same decoder, and it
registers a remembered remover directly so it never takes a ref number.

What the vectors pin about those functions:

- `resolve` calls `active`, then `iconAccent`, then `enabled`, then `visible`, each with a fresh
  context, even for a command in a disabled category (`categories`).
- A throw falls back: `enabled` and `visible` to true, `active` to false, the accent to none
  (`throwing-derivations-fall-back`, `icon-accent`).
- A command with `run` never gets the surface routing.
- `listCommands` and `searchCommands` walk the ids registered when the call began. A derivation
  that removes a command ahead of the walk makes it skip that one, and one it registers is not
  visited (`re-entrant-registry`, `unregister-behind-a-search`).
- An explicit empty document id is kept as the context's `documentId`, but because it is falsy the
  TypeScript resolves through `ctx.get` against the active document. `CommandCtx::get_any` does
  the same.

## JavaScript semantics

The registry is a `Map`: iteration is by insertion, `replace` keeps an id's place, and an id
removed and registered again moves to the end. A remover deletes its id only while its own entry
is still the one registered. `menu` and `modal` are truthiness checks in the TypeScript, so an
empty id is no target, while an empty `panel` string is still a panel. `search` trims with the
ECMAScript whitespace set (`epdf_ui_core::js::trim`) and lowercases the query and the label but not
the id.

After `destroy`, a capability reference someone kept still reads and changes the registry,
matches strokes, resolves and executes, against the last state the slice had; labels fall back to
their keys because i18n no longer resolves. Category intents are dropped, and the first one is
reported `instance-closed` (`after-destroy`).

## Coverage and what is not ported

The commands suites reach every statement and branch of the package: 575 of 575 statements and 164
of 164 branches.

Answers outside the TypeScript types are not ported. A derivation typed `boolean` that returns
something else would reach the resolved command through `&&` unchanged when falsy (`enabled: 0`),
and Rust's derivations return `bool`; the harness reads an answer by JavaScript truthiness. An
accent object with members beyond `primary` and `secondary` loses them here. A derivation that
returns a promise is truthy in the TypeScript; Rust's derivations are synchronous. The harness
prints a `getCommand` definition's data members only; no vector reads one that has functions,
which the TypeScript would print as refs.

`resolvedCommandsEqual`'s `a === b` shortcut has no Rust counterpart. Every member it compares is
a string, a boolean or a list of strings, so an object always equals itself by value, and
`resolvedCommandsEqualSelf` pins that.

## Mutation testing

`scripts/mutants.sh -p epdf-plugin-commands --test-package epdf-plugin-commands --file <file>`,
file by file over every module but `harness.rs`, catches 107 of 108 viable mutants (107 caught, 0
timeouts; 25 unviable). No new cases were needed. The one survivor is in `src/contract.rs`:
`<impl fmt::Debug for CommandDef>::fmt` replaced with `Ok(Default::default())`. The impl exists
because `run` holds a closure that cannot derive `Debug`. The TypeScript has no counterpart, and
nothing in the crate formats a definition, so no vector can see its output.
