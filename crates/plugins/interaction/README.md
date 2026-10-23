# epdf-plugin-interaction

`packages/plugin/interaction` in Rust, at upstream `b456ee4fd`: the pointer hub (tools with their
own page-space pointer members, a tool stack, prioritized handlers, gesture capture and gesture
events, cursor claims, touch pre-flight), its settings, and the feedback plugin. All 19 cases in
`vectors/plugins/interaction-functions.json` and all 27 in `vectors/plugins/interaction.json`
replay identically, with no member-order allowance (`cargo test -p epdf-plugin-interaction`).

The modules follow the TypeScript files: `contract.rs`, `model.rs`, `controller.rs`, `state.rs`
and `feedback.rs`. The plugin is document-scoped, built through `create` with no reducer, and
declares its settings (`defaultTool`, `tools`) as `definePlugin({ settings })` does. The state holds
`activeToolId`, `toolStack` and `cursor`; every change goes through the pure transitions in
`model.rs`. The capability starts with the settings calls (`get_settings`, `update_settings`,
`reset_settings`, `on_settings_changed`), then `getActiveTool`, `getActiveToolId`,
`getDefaultToolId`, `listTools`, `getTool`, `hasTool`, `activateTool`, `activateDefaultTool`,
`pushTool`, `popTool`, `setToolCursor`, `registerTool` (which errs `conflict` on a taken id unless
`replace`, and moves a replaced id to the end), and the events `onToolChanged`, `onGestureStarted`,
`onGestureEnded` and `onGestureCancelled`. An unknown tool errs `not-found`. Both errors are
`PluginError`s with capability `interaction`.

A tool given to `update_settings` can carry callbacks, which the kernel's JSON settings store can't
hold. `settings.rs` stores it as a handle into a table that belongs to the registration, so it keeps
its identity; a tool written as plain JSON reads as one without callbacks. Every change to the store
prunes the handles neither the current nor the registered settings name, a JSON write through
`kernel.settings_of` included (`a_json_settings_write_releases_earlier_typed_tools`).

Settings tools compare by identity, which is a Rust-only divergence. `encode` in `settings.rs:58-68`
writes each tool as its handle, so two distinct tools with equal data and no callbacks are a change.
The TypeScript's `sameValue` (`packages/core/main/src/settings.ts:187-222`) deep-compares a plain
tool given without `enables` and calls that no change; one with an `enables` set it compares by
identity, as Rust does. Rust can't follow the first case because `Tool.enables` can't tell a tool
given without `enables` from one given an empty set. Where the two differ, Rust fires
`on_settings_changed` and swaps the tools, and the swap's removers drop those ids'
`set_tool_cursor` entries. `distinct_equal_settings_tools_count_as_a_change` pins it.

## Coverage

The coverage figures below were measured before upstream replaced the reducer with transitions,
and are stale until the coverage refresh. The generators reach 589 of the package's 590 statements and 131 of its 132 branches. The one line
they can't reach is `model.ts:47`, the `default` arm for an action outside the four-member
`InteractionAction` union. In Rust that union is an enum, so the arm has nothing to match.

This port added four scenarios to `src/suites/interaction.ts` (`tool-lookups`,
`push-and-pop-tools`, `register-tool-conflict`, `gesture-events`) and four reducer cases for the
tool stack, which is what took the package from 197 of 207 controller statements to all of them.

## The host lens

The TypeScript hands out one object under one token and types it twice: `InteractionCapability`
for applications, `InteractionHostCapability` for pointer sources and feature plugins. Rust has one
type, `InteractionCapability`, and `InteractionHostCapability` is an alias for it. The host members
are `active_tool_enables`, `dispatch_pointer`, `would_claim_touch`, `register_handler` (taking
`RegisterHandlerOptions`), `claim_cursor`, `has_cursor_claim`, `get_cursor` and
`on_cursor_changed`.

`InteractionHandler` is a trait, and it's what the Rust selection, annotation, form and stage
plugins implement. The TypeScript interface has six optional members; the trait gives each one a
default that is exactly what the hub does when the member is absent:

- `on_move`, `on_up` and `on_hover` do nothing;
- `on_cancel` calls `on_up`, the `onCancel ?? onUp` fallback in `dispatchPointer`;
- `claims_touch` answers false.

So a handler implements only the members it uses. `tests/api.rs` pins those defaults, since no
vector can: a scripted handler in a vector always overrides them.

Registration returns `Unregister`, which can be called any number of times, as the TypeScript
closure can. `registerTool`'s remover owns its registration only: once a `replace` put another tool
under the id, the old remover does nothing (`register-tool-conflict`). The TypeScript compares
object identity; Rust keeps a serial per registration.

## Where the port differs in shape

Handler methods are infallible. A TypeScript handler that throws aborts `dispatchPointer` mid-pass
and the throw reaches the caller. No shipped handler throws, and a `Result` on every member would be
noise for every downstream implementation, so the vectors don't script a throwing handler.

`Phase` is a four-variant enum. The TypeScript routes any phase string that isn't `down`, `move` or
`cancel` down the `up` branch; a Rust caller can't construct one.

The hub releases its handler list, its source scopes, the tools' own handlers and the gesture owner
when the instance closes. The TypeScript doesn't need to: a garbage collector reclaims the cycle
between the hub and the handlers that hold it. Rust would leak it. A closed hub then routes nothing
(`closing_the_document_releases_the_handlers` in `tests/api.rs`). The tool registry stays, so a tool
whose callbacks hold the capability strongly still keeps the hub alive; give such a callback a weak
handle.

The vectors write every page ref as `{kind: 'objectNumber', pageObjectNumber}`, a spelling the
current `PageRef` doesn't have (`tooling/plugin-conformance/src/suites/interaction.ts:88`). The
TypeScript reads `objectNumber` of such a ref as `undefined`, so `samplePointOn` matches no page and
the vectors record `null` for the same-page cases. `harness.rs` reads such a ref as a stand-in that
names no page and prints it as given. `sample_point_on_falls_back_to_the_same_page_hit` pins what a
real ref does.

`harness.rs` exports the JSON bridges (`sample_from_json`, `sample_json`, `with_vector_sample`,
`tool_from_json`, `tool_json`, `page_ref_from_json`, `page_ref_json`, `gesture_json`,
`tool_changed_json`, `config_from_json`, `ScriptedHandler`) and the `Interaction` adapter, which
downstream crates register next to their own when their vectors drive the hub. A vector's sample,
tool or unreadable page ref prints as the vector gave it, because the TypeScript passes those
objects through.

## Mutation testing

These counts predate Task 13.11's re-port and are stale until Task 14 runs the table again.
`scripts/mutants.sh -p epdf-plugin-interaction --test-package epdf-plugin-interaction --file
<file>`, file by file over every module but `harness.rs`, catches 87 of 90 viable mutants (87 caught,
0 timeouts; 26 unviable).

The three survivors are all in `src/controller.rs:197`, the `Greater` arm of the priority
comparator in `eligible`: `>` becoming `==`, `<` or `>=`. `sort_by` is a stable sort that only asks
whether the comparator returned `Less`. `Greater` and `Equal` both leave a pair in registration
order, so every one of the three orders handlers the same way.
