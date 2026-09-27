# epdf-plugin-interaction

`packages/plugin/interaction` in Rust, at the v3 contract: the pointer hub (tools, a tool stack,
prioritized handlers, gesture capture and gesture events, cursor claims, touch pre-flight) and the
feedback plugin. All 27 cases in `vectors/plugins/interaction.json` and all 19 in
`vectors/plugins/interaction-functions.json` replay identically (`cargo test -p
epdf-plugin-interaction`).

The modules follow the TypeScript files: `contract.rs`, `model.rs`, `controller.rs` and
`feedback.rs`. The plugin is document-scoped and built through `create`, as
`definePlugin({ create })` builds it. The api uses the v3 names: `getActiveTool`,
`getDefaultToolId`, `listTools`, `getTool`, `hasTool`, `activeToolEnables`, `activateTool`,
`activateDefaultTool`, `pushTool`, `popTool`, `setToolCursor`, `registerTool` (which errs `conflict`
on a duplicate id unless `replace`), and the events `onToolChanged` (with the caller's `payload`),
`onGestureStarted`, `onGestureEnded` and `onGestureCancelled`. An unknown tool errs `not-found`.
Both errors are `PluginError`s with capability `interaction`.

## Coverage

The generators reach 589 of the package's 590 statements and 131 of its 132 branches. The one line
they can't reach is `model.ts:47`, the `default` arm for an action outside the four-member
`InteractionAction` union. In Rust that union is an enum, so the arm has nothing to match.

This port added four scenarios to `src/suites/interaction.ts` (`tool-lookups`,
`push-and-pop-tools`, `register-tool-conflict`, `gesture-events`) and four reducer cases for the
tool stack, which is what took the package from 197 of 207 controller statements to all of them.

## The host lens

The TypeScript hands out one object under one token and types it twice: `InteractionCapability`
for applications, `InteractionHostCapability` for pointer sources and feature plugins. Rust has one
type, `InteractionCapability`, and `InteractionHostCapability` is an alias for it. The host members
are `dispatch_pointer`, `would_claim_touch`, `register_handler` (taking `RegisterHandlerOptions`),
`claim_cursor`, `get_cursor` and `on_cursor_changed`.

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

The hub releases its handler list, its source scopes and the gesture owner when the instance
closes. The TypeScript doesn't need to: a garbage collector reclaims the cycle between the hub and
the handlers that hold it. Rust would leak it.

`harness.rs` exports the JSON bridges (`sample_from_json`, `sample_json`, `tool_json`,
`tool_changed_json`, `config_from_json`, `ScriptedHandler`) and the `Interaction` adapter, which
downstream crates register next to their own when their vectors drive the hub.

## Mutation testing

`scripts/mutants.sh -p epdf-plugin-interaction --test-package epdf-plugin-interaction --file
<file>`, file by file over every module but `harness.rs`, catches 87 of 90 viable mutants (87 caught,
0 timeouts; 26 unviable). One of those is killed by `a_nullable_page_ref_round_trips` in
`tests/api.rs`, checked by applying it by hand: `page_ref_json::nullable::deserialize` replaced with
`Ok(None)`. Only `GestureEvent` uses that module, and it only serializes, so no vector decodes one.

The three survivors are all in `src/controller.rs:197`, the `Greater` arm of the priority
comparator in `eligible`: `>` becoming `==`, `<` or `>=`. `sort_by` is a stable sort that only asks
whether the comparator returned `Less`. `Greater` and `Equal` both leave a pair in registration
order, so every one of the three orders handlers the same way.
