# epdf-plugin-form

`packages/plugin/form` in Rust: the field plane over the engine's `forms.*` services, the fill
projection, value writes and reset, interchange, field design with the draw-to-place handler, ISO
field selection, and the K/V/C/F scripting transaction. All 191 cases in five vector files replay
identically (`cargo test -p epdf-plugin-form`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/form-core.json` | function | 87 | `core/model.ts`, `core/fill-items.ts`, the reducer, the tool table, `resolveFieldSelection` and `buildSubmitEntries` |
| `vectors/plugins/form-fill.json` | scenario | 23 | hydration and refresh, widget geometry, the fused fill projection, typed writes and their queue, reset with and without a batch door, interchange, the twins, the reactive effect |
| `vectors/plugins/form-design.json` | scenario | 24 | tool and behavior registration with and without the annotation plugin, `placeField` and the other design verbs, the place handler as `PointerSample` sequences |
| `vectors/plugins/form-actions.json` | scenario | 13 | the `reset-form` executor, the submit resolver, the script commit sink, widget activation, the widget event feed |
| `vectors/plugins/form-scripting.json` | scenario | 44 | the standalone controller (environment, boot, keystroke, validate, calculate, format, effect canonicalisation, budgets, faults) and the capability over the actions realm |

The scenario registry runs the real interaction, actions and annotation adapters beside this one.

## The API stamp and siblings use

The TypeScript's root, `/contract`, `/contract/host` and `/internal` entries are type lenses over
one runtime token. Here that's `FORM` and `FormCapability`, whose host members
(`reset_form_action`, `resolve_submit_dataset`, `commit_script_form_effects`) say so in their docs.
The methods are the TypeScript's in snake case: `snapshot`, `fill_items`, `set_text`,
`commit_value`, `activate_widget`, `place_field`, `can_design`, and so on. `refresh()` is also
`refresh_with(force)`, since the TypeScript's `refresh` takes the flag too.

`/scripting` is `create_form_scripting_controller(FormScriptingControllerOptions::Config { doc,
document, config })`, which is what stamp calls for its detached asset documents, then
`recalculate()` and `dispose()`. `FormCommitResult`, `FormUiEffect` and `FormScriptingOptions` are
the result and configuration types. `FormScriptingOptions::sandbox_factory` is required: the
TypeScript falls back to a lazily imported QuickJS sandbox, and Rust has no default. A host with no
sandbox builds no controller, and inside a viewer the capability then runs exactly as the
TypeScript does with scripting disabled (`no-sandbox-host`).

Engine DTOs (snapshots, fields, effects, action trees, annotation refs) stay `serde_json::Value`.
Fill items are typed, but members copied from the field DTO stay JSON so an absent one prints as
absent.

## Host inputs

The TypeScript reads three ambient values, and each comes in as an input:

- `console.warn('[form] form snapshot failed to load', err)` and `console.error('[form] placeField
  failed:', err)` go to `FormHost::console_warn` and `console_error` on `FormPluginConfig`. The
  vectors log both as callbacks.
- `-new Date(nowMs).getTimezoneOffset()` in the standalone realm is
  `FormScriptingOptions::host_utc_offset_minutes`, default zero. The generator pins `TZ=UTC`.
- `Date.now()`, for the standalone clock and the transaction's execution budget, is `rt::now()`.

## Where the port has to be careful

Turn counts are behaviour, and each `await` and `.then` is mirrored with `src/promise.rs`: an
engine call awaited resumes three turns after it settles, `.then` on one reacts after one, and an
`async` function that returns a promise settles two turns after it. The snapshot load is
`list().then(apply).catch(warn).finally(clear)`, which settles two turns after its `finally`
callback. `form-design`'s `queued-verbs-interleave-with-page-reloads` pins how unawaited annotation
reloads interleave with the next queued verb.

Keystroke splicing counts UTF-16 code units and reads offsets the way `String#slice` does. Output
budget bytes are the UTF-8 length of `JSON.stringify(output)`, so `value::json_stringify` prints
numbers as `Number#toString` does rather than as `serde_json` does.

`sameAnnotationRef` compares an index ref's `revision` with `===`. A `RevisionToken` object read
from JSON is never identical to another, so the Rust treats an object revision as unequal and a
primitive one by value. Activation reads a loaded annotation through the annotation plugin's typed
`AnnotationRef`, and a ref that doesn't decode (a string revision, say) counts as not loaded.

One harness change came with this port: a fake document's `lacks` list removes the optional
`forms.applyEffects` member (commit `5c6ad7125`). Without it no vector could reach the six branches
that handle an engine without a batch door.

## Where the port differs in shape

A few malformed DTOs make the TypeScript throw a `TypeError` where the Rust carries on, and no
vector sends one: a snapshot without `calculationOrder` (read as empty), a form event whose `origin`
is `null` (ignored), a field without `flags` (not read-only). A non-boolean `flags.readOnly` prints
as `disabled: true` where the TypeScript's `||` would print the value itself. `writeDone` past the
end of the field list pads with `null`s, which JSON prints as the TypeScript prints its holes, and
the index skips them as `forEach` skips holes.

## Coverage

The generators reach 2,746 of the package's 2,762 statements and 756 of its 799 branches
(`vectors/coverage.json`). Every file outside `form.plugin.ts`, `handler.ts` and `scripting.ts` is
at every statement. The 16 statements left, and the branches with them, guard states the public
surface can't produce:

- `form.plugin.ts` 117 to 128, 135, 145 and 146: the sink, resolver and executor closures when
  `ctx.tryGet(FormToken)` finds no form capability, and the executor's non-`reset-form` guard. The
  plugin registers them only while it exists, and only for `reset-form` nodes.
- `handler.ts` 135: a click placement that isn't a box. `resolveClickPlacement` returns a box for
  every width-and-height policy, which is all the palette has.
- `scripting.ts` 367 and 368: a target with no script view. The overlay is built from the same
  snapshot the target was found in.

The unreached branches are the `if (!doc)` and `ctx.doc?` arms (the plugin is document-scoped), the
`String(error)` arm of every `error instanceof Error ? ... : String(error)` (the fake engine and the
scripted sandbox only throw `Error`s, and Rust has nothing else), `scripting.ts:76-77` (the QuickJS
default, which doesn't port), `scripting.ts:86` (a null document meta), `scripting.ts:177` (a
script value change on a pushbutton or signature, which every path throws on first),
`scripting.ts:213` (an overlay field missing from the original it was cloned from),
`form.plugin.ts:98` (`activeTool()` is never undefined), `form.plugin.ts:155` (a failed reset never
carries an error), and `handler.ts` 60, 77 and 83 (a move or up without a captured gesture, or with
a non-palette tool, which the hub never routes).

## Mutation testing

`scripts/mutants.sh -p epdf-plugin-form --test-package epdf-plugin-form --file <file>`, file by file
over every module but `harness.rs`, catches 454 of 466 viable mutants (449 caught, 5 timeouts; 164
unviable). Two of the 449 are `scripting.rs` survivors that later cases kill, checked by applying
each by hand; the other numbers are from the last run of each file. The 12 survivors are
equivalent:

- `capability.rs:396` and `:418`, the fill caches' grant comparison as `!=`. The grant can't change
  while the model stands still, so a miss recomputes the same items.
- `capability.rs:745`, reloading a changed widget's page 0: the annotation plugin finds no crop for
  it and makes no call.
- `capability.rs:836`, `no_tree`'s first `&&` as `||`. Every inert step carries a diagnostic, so an
  inert result with steps never has none.
- `scripting.rs:637`, dropping the null check on `/CO` entries. A null matches only a field whose
  FQN is literally `undefined`, and then only as a recalculation anchor, which proposes nothing.
- `field_selection.rs:86` and `:92`, `explicit_include`'s `&&` and `explicit`'s `&&` as `||`.
  `listed()` is false for every selected field whenever `explicit_include` is false.
- `handler.rs:74` (two), the handler's `id`, which the hub never reads, and `handler.rs:84`,
  `enabled_for` answering true, which only reaches an `on_down` that declines any tool the palette
  doesn't know.
- `core/model.rs:59` and `reducer.rs:11`, `initial_model` and `initial_form_state` as
  `Default::default()`: the same values.

The timeouts count as caught: four `value.rs` mutants of `get` and `num` hang the replay, and
`auto_name`'s `+=` as `*=` stays at 1 and never finds a free name.
