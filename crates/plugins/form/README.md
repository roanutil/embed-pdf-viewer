# epdf-plugin-form

`packages/plugin/form` in Rust at the v3 contract: the field plane over the engine's `forms.*`
services, the fill projection, value writes and their batch doors, reset, interchange, field design
with the draw-to-place handler, ISO field selection, and the K/V/C/F scripting transaction. All 216
cases in five vector files replay identically (`cargo test -p epdf-plugin-form`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/form-core.json` | function | 100 | `core/model.ts` (with `widgetAt`), `core/fill-items.ts`, the reducer, the tool table, `fieldRef`, `resolveFieldSelection` and `buildSubmitEntries` |
| `vectors/plugins/form-fill.json` | scenario | 30 | hydration, `getStatus` and `refresh`, widget geometry, the fused fill projection, the field reads, typed and batch writes, `reset` and `resetAll`, interchange, the twins, the six confirmed-change events |
| `vectors/plugins/form-design.json` | scenario | 28 | tool and behavior registration with and without the annotation plugin, `createField` and the other design verbs, the hit test, the place handler as `PointerSample` sequences |
| `vectors/plugins/form-actions.json` | scenario | 13 | the `reset-form` executor, the submit resolver, the script commit sink, widget activation, the widget event feed |
| `vectors/plugins/form-scripting.json` | scenario | 45 | the standalone controller over a detached realm, and the capability over the actions plugin's realm |

The scenario registry runs the real interaction, actions and annotation adapters beside this one.

## Shape

The files follow the TypeScript package. `controller.rs` is the composition root; each area is an
`impl` block on the one `FormCapability`, one file per TypeScript file under `services/`, `read/`,
`sync/`, `write/`, `scripting/` and `tools/`, and their private state sits in `Core`. `form_plugin`
declares `create`: the controller is built during bring-up and `connect` subscribes to the document
events, starts hydration and wires the hub.

The public surface is the v3 contract in snake case. Fields are addressed by a `FormFieldRef`
(`contract::field_ref` builds one) and pages by the kernel's `PageRef`: `get_snapshot`,
`list_fields`, `get_value`, `export_values`, `list_widgets`, `get_widget_at`, `set_value`,
`set_value_raw`, `set_values`, `import_values`, `reset`, `reset_all`, `create_field`,
`attach_widget`, `detach_widget`, the twins and the six `on_*` hooks. The host lens
(`list_fill_items`, `get_fill_item`, `ensure_loaded`, `get_page_box`, `notify_widget_event`,
`reset_form_action`, `resolve_submit_dataset`, `commit_script_form_effects`) is on the same type,
as the TypeScript's two lenses share one token. `FormHostCapability` is an alias.

The scripting controller no longer builds a realm. `create_form_scripting_controller` takes a
`TransactionPort` and a budget: the capability hands it the actions plugin's `script_realm`, and a
host evaluating a document that is not displayed (stamp's asset documents) mints one with
`create_detached_script_realm`. `FormScriptingOptions` and the sandbox factory are gone with the
standalone realm. `services::scripting::commit_surface` is public so a detached-realm owner can
surface a result through the actions port.

Engine DTOs (snapshots, fields, effects, action trees, refs) stay `serde_json::Value`. Fill items
are typed; members copied from the field DTO stay JSON so an absent one prints as absent.

## Host inputs

`console.warn('[form] form snapshot failed to load', err)`, `console.error('[form] createField
failed:', err)` and `console.error('[form] event listener failed:', err)` go to
`FormHost::console_warn` and `console_error` on `FormConfig`. The vectors log each as a callback.
`Date.now()`, for the transaction's execution budget, is `rt::now()`.

## Where the port has to be careful

Turn counts are behaviour, and each `await` is mirrored with `src/promise.rs`. `refresh` is the
TypeScript's `do { await readSnapshot() } while (refreshAgain)` inside a `.finally`, whose derived
promise settles two turns after its callback; every call during a run shares that promise and
queues one more read. `write`, `reset` and `resetAll` are `async` functions that return the queue's
promise, so they cost the adoption turns.

Keystroke splicing counts UTF-16 code units, and output budget bytes are the UTF-8 length of
`JSON.stringify(output)`, printed the way `Number#toString` prints numbers.

`sameAnnotationRef` compares an index ref's `revision` with `===`. A revision object read from JSON
is never identical to another, so the Rust treats an object revision as unequal.

## Coverage

Across the whole corpus the generators reach 3,431 of the package's 3,460 statements and 879 of
its 927 branches (`vectors/coverage.json`). The 29 statements left:

- `controller.ts` 85 to 91: `createFormCapability`, the unit-test door that builds and connects a
  controller outside the plugin. Nothing in the plugin calls it.
- `form.plugin.ts` 137 to 147, 164 and 165: the sink and the executor when `ctx.tryGet(FormToken)`
  finds no form capability. Both are registered only while the plugin lives.
- `write/activation.ts` 27 to 31: `sameAnnotationRef`'s `nm` and `index` arms. `activateWidget`
  finds the field by the widget's object number, so a ref of another kind rejects `not-found`
  first, and a listed candidate of another kind fails the kind check on line 23.
- `scripting/controller.ts` 142 and 143: a target with no script view. The overlay is built from
  the snapshot the target was found in.
- `scripting/overlay.ts` 94: `formValueFromScriptValue` for a pushbutton or signature, which every
  path refuses first.
- `tools/handlers.ts` 126: a click placement that is not a box. Every palette policy is a box.

The unreached branches are the `if (!doc)` and `ctx.doc?` arms (the plugin is document-scoped), the
`String(error)` arm of every `error instanceof Error ? ... : String(error)` (the fake engine and the
scripted sandbox only throw `Error`s), `this.transactionBudget ?? DEFAULT_SCRIPT_BUDGET` (a realm
port always carries a budget), `getActiveTool()?.enables ... ?? false` (there is always an active
tool), `result.effectsResult?.results ?? []` in `resetAll` (a failed reset always carries its
effects result), the handler's guards against a move or up without a captured gesture, the
`sameAnnotationRef` arms above, and a design verb's widget without a page.

## Pinned TypeScript behaviour

`importValues` addresses each field as `{kind: 'fqn', name}`. With scripting on, the transaction
matches fields by `refKey`, so a field that has an object number never matches its own name and
the write fails "Form field 'fqn:email' no longer exists" (`form-scripting`,
`batch-writes-count-rejections`). Without scripting the engine resolves the name and the write
lands.

`activateWidget` takes the widget alone and can only resolve an object-number ref; an `nm` or
`index` widget ref rejects `not-found`.

## Where the port differs in shape

A few malformed DTOs make the TypeScript throw a `TypeError` where the Rust carries on, and no
vector sends one: a snapshot without `calculationOrder` (read as empty), a form event whose `origin`
is `null` (ignored), a field without `flags` (not read-only). `writeDone` past the end of the field
list pads with `null`s, which JSON prints as the TypeScript prints its holes.

## Mutation testing

`scripts/mutants.sh -p epdf-plugin-form --test-package epdf-plugin-form --file <file>`, file by
file over every module but `harness.rs`, catches 521 of 530 viable mutants (519 caught, 2 timeouts;
186 unviable). The run itself, at `afb842b48`, caught 499 and missed 29. Cases added afterwards
kill 20 of those 29, and the counts above include them: 13 through new or reshaped vectors in
`form-core`, `form-fill`, `form-design` and `form-scripting`, 7 through unit tests. Of those 7, 3
are the fill projection's `Rc` identity (`tests/api.rs`), which JSON cannot show, and 4 are
`value::f64_of` and `value::string_to_number` (the `value.rs` tests), which nothing in the crate
calls. Each kill was confirmed by applying the mutant by hand and watching `cargo test` fail, not
by a second run. The 2 timeouts are `+=` to `*=` in `auto_name` (`write/fields.rs:55`, a loop that
never ends) and deleting `!` in `refresh` (`sync/hydration.rs:53`). The other 9 are equivalent or
unreachable:

| File | Line | Mutant | Why no case kills it |
|---|---|---|---|
| `core/model.rs` | 59 | `initial_model` to `Default::default()` | Equivalent. It is `Model::default()`. |
| `model.rs` | 33 | `initial_form_state` to `Default::default()` | Equivalent. `ResourceStatus` defaults to `Idle` and `Rc<Model>` to the default model, the value the function builds. |
| `field_selection.rs` | 86 | `&&` to `\|\|` in `explicit_include` | Equivalent. `explicit` also needs `listed(field)`, and no selected field is listed unless the targets are an include list: with no targets nothing is listed, and in exclude mode the selection is exactly the unlisted fields. |
| `field_selection.rs` | 92 | `&&` to `\|\|` in `explicit` | Equivalent, for the same reason: in include mode every selected field is listed, otherwise none is. |
| `lib.rs` | 107 | `defer` to `()` | Unobservable. It unregisters the commit sink, the submit resolver and the `reset-form` executor from the actions plugin at document close, and the actions plugin is document-scoped too, so its registries close with the same document. |
| `write/activation.rs` | 41 | `==` to `!=` on either `Some("index")` (2 mutants) | Unreachable. `activateWidget` finds the field through `widgetObjectOf`, which reads 0 for any ref that is not an object number, and 0 is never indexed, so an index ref rejects `not-found` before any page read. |
| `write/activation.rs` | 41 | the first `&&` to `\|\|` | Equivalent. The arm runs only after `eq("kind")`, so both kinds are the same and `l == index \|\| r == index` equals `l == index && r == index`. |
| `write/activation.rs` | 150 | `&&` to `\|\|` in `no_tree` | Equivalent. The dispatcher answers a non-`inert` status with no steps only from its catch arm, which always pushes a `trigger-failed` diagnostic, so `diagnostics.is_empty()` decides either way. |

The case `a-kindless-address-matches-no-candidate` (`form-scripting`) sends the one address that
reaches the last arm of `same_annotation_ref` without being an index ref, an off-contract
`{ page, annotObjectNumber }` with no kind, and kills the other two `&&` mutants on line 41.
