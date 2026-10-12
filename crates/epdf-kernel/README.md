# epdf-kernel

`packages/core/main` in Rust: the kernel, its store, the plugin plan, resource scopes, the document
lifecycle and the v3 controller contract. All 96 cases in `vectors/kernel/lifecycle.json` and all
70 in `vectors/kernel/utils.json` replay identically (`cargo test -p plugin-conformance`), and
`tests/runtime.rs` pins what a vector can't see.

## Writing a plugin against the v3 contract

A v3 plugin declares `create` instead of `capability` and `effects`, as the TypeScript does with
`definePlugin({ create })`:

```rust
define::<MetadataState, MetadataAction>("metadata")
    .document_scoped()
    .initial_state(MetadataState::default)
    .reduce(reduce_metadata)
    .create(&METADATA, |cx: Controller<MetadataState, MetadataAction>| {
        let api = MetadataApi::new(cx.clone());
        let connect: ConnectFn = Rc::new(move || {
            let doc = cx.doc()?;
            cx.listen(doc.subscribe(Rc::new(|event| { /* publish */ })));
            Ok(())
        });
        Ok((api, Some(connect)))
    })
    .build()
```

The kernel builds a document plugin's controller eagerly during bring-up, right after that plugin's
effects, in dependency order, and runs its `connect` there. A workspace controller is built when the
kernel is constructed and connects at `start()`, after its effects. A throw from either fails the
open or the start, exactly as it does in TypeScript.

`Controller<S, A>` derefs to `ControllerContext`, which derefs to `PluginContext`. The members map
one to one onto `controller.ts`:

| TypeScript | Rust |
|---|---|
| `ctx.instanceId` | `instance_id()`: `<doc>#<n>`, or `workspace:<id>` |
| `ctx.doc` | `doc()`: the guarded handle, `Err` for a workspace plugin |
| `ctx.events.source()` | `event_source::<T>()`, disposed with the instance |
| `ctx.listen(source, fn)` | `listen(unsubscribe)`: subscribe yourself, hand over the unsubscribe |
| `ctx.waitFor(pred, {signal})` | `wait_for(pred, signal)` |
| `ctx.serialQueue(key)` | `serial_queue(key)` |
| `ctx.latest(key)` | `latest(key)`: a `LatestLane`; `run` takes a function returning a `Promise` |
| `ctx.acquire(get, dispose)` | `acquire(get, dispose)` |
| `ctx.geometry.forPage(ref)` | `for_page(PageRef)` and `try_for_page(PageRef)`, returning a `PageSpace` |
| `ctx.assertPageRef(ref)`, `ctx.getPage(ref)` | `assert_page_ref`, `get_page` |
| `ctx.tryForDocument(token, id)` | `try_for_document(&TOKEN, id)` |

Errors use the one vocabulary in `error.rs`: `plugin_error(code, capability, message, cause,
details)` builds a `PluginError`, `to_plugin_error(capability, &error)` is the boundary mapping, and
`Error::class` carries what the TypeScript tells apart with `instanceof` (`CancelledError`,
`PermissionDenied`, `PluginError`). A table error that only carries one of those names is a plain
error, as it is in TypeScript.

State goes through a `SliceLease`. Once an instance closes, a retained context's dispatch is
dropped and reported once, and its reads answer the last state. Resolving a token a plugin did not
declare in `requires` or `optional` is refused. The TypeScript only does that in development, but
the vectors are generated in development, so the Rust kernel always does.

This port added three turn rules. `Promise::race` in `rt.rs` adopts an abortable input as a
thenable (three turns, not one), which reordered two concurrent opens in `open-all-returns-ids`
until it did. The guarded handle in `guarded.rs` reads an abortable engine call the same way. And
a promise an `async` function returns costs the adoption turn (`documents.retry`).

## Deliberate differences

`ctx.geometry` doesn't cache page spaces. The TypeScript caches one per registry revision and page,
which nothing can observe, and building one is six arithmetic operations. `PageSpace` and `PageRef`
live in `geometry.rs` rather than in `epdf-geometry`, because that crate depends on this one for its
conformance harness and the dependency can't run both ways.

## Coverage

The kernel suites reach 2,804 of the package's 2,826 statements and 675 of its 695 branches. Four
of the missing statements are `definePlugin` in `index.ts`, which the plugin suites call. The rest
cannot be reached from the public surface:

- `controller.ts:44`, the `not-ready` refusal for a session without a handle. A controller is only
  built during bring-up or later, and the handle is set before bring-up begins.
- `controller.ts:65`, clearing the page-space cache past 4,096 entries.
- `controller.ts:108`, `waitFor` finishing twice. Every path that finishes removes the others first.
- `store.ts`: the lease's `live` getter and the deprecated `registerSlice`, `removeSlice` and
  `dispatchTo`, which the kernel no longer calls; replacing a live lease under the same key, since
  close revokes first; and the loop that deletes states after every lease was revoked.
- `lanes.ts:88`, a run that stops being current while it aborts the previous one, and the
  `?? 'superseded'` arms at 90 and 94: an aborted lane signal always carries a reason.
- `kernel.ts`: the default `report` sink (the harness always passes one), the refused-reservation
  fallbacks in `openAll` for a thunk and for an input without an id (both get a fresh ticket, so
  they can't collide), and the four arms the previous port already recorded.
- `context.ts:93`, the workspace arm of the no-lease message, and `env.ts:15`, the `catch` in
  `isDev`.

## Mutation testing

`scripts/mutants.sh -p epdf-kernel --test-package epdf-kernel --test-package plugin-conformance --file <file>`,
file by file over every module, catches 613 of 644 viable mutants (608 caught, 5 timeouts; 361
unviable). The run measured only the kernel's own tests and vectors, not the plugin crates that
also exercise it. 67 of those kills come from new tests, each checked by applying the mutant by
hand. Five more mutants no longer exist, because the code they mutated was wrong and is rewritten.

The file and line numbers in this section are as of `76ae3b597`, the commit that recorded the run.
The code has moved since: `kernel.rs:419`, for one, was line 481 when this note was added.

The new vectors in `kernel.ts`:

- `a-rekey-onto-a-closing-id-refuses`: a ticket whose input names a document still tearing down.
  Only `sessions.has` knows the id then, since the slot has already left the store. Kills `||` to
  `&&` at `kernel.rs:419`.
- `a-rekeyed-ticket-lists-its-new-id-while-the-engine-opens` lists the slot between the re-key and
  the engine's answer. Kills dropping `id` from the re-keyed `PendingMeta` (`kernel.rs:432`).
- `page-space-at-the-origin`: `describeSpace` now also maps the origin both ways, and the case
  adds a crop whose top is 0 and registry refs of 1.5 and -1. Kills `geometry.rs:33` (both `&&`),
  `:83` (both `*` to `/`) and `:90:36`, each of which turns a zero into NaN or names a page the
  TypeScript does not.
- `lane-cancel-reasons`: `latestCancel` with null, numbers, `true`, `{}` and a nested array, plus
  a `latestRunning` read after a superseded run finishes. Kills both `lanes.rs:113` mutants, the
  `$undefined` guard replaced with `true` (`:40`), and `AbortSignal::same` replaced with `true`.
- `guarded-handle-security-reads`: `docRead` now calls a method at the end of its path, so the
  probe reads identity, current, the password prompt, the three annotation checks and
  `events.lastServerId` through the guard, on two documents with opposite answers. Kills 11
  mutants in `guarded.rs` and both `Some` replacements of the default `last_server_id` in
  `engine.rs`.

The lane case exposed a port bug. `js_string` printed integers through `x as i64`, so a reason of
`1e20` read as `9223372036854775807`, and it never used exponent notation (`1e+21`, `1.5e-7`). It
also printed `undefined` inside an array, where `Array.prototype.join` prints nothing. It now
uses `js::number_to_string`, a copy of the one in `epdf-geometry`, which depends on this crate and
so cannot be depended on. The five mutants of the old integer test at `lanes.rs:29` went with it.

The rest are Rust tests, because they reach what the TypeScript has no counterpart for.
`plugin-conformance/tests/kernel.rs` drives a typed document plugin and a typed workspace plugin
through a real kernel, which kills every survivor in `plugin.rs`, `Kernel::try_capability`,
`Kernel::core`, `dyn Action::downcast` (through the reducer) and `LifecycleEvents::dispose`.
`tests/runtime.rs` adds three `Set` iteration cases for the store's listener lists
(`store.rs:59`, `:66`, `:91`), lease accessors, `get_slice` and `dispatch_to`, a stub handle for
the guard's `has` and `last_server_id` pass-throughs, `listener_count`, `Error::from_spec`
members and `PageRef::to_json`. The `js.rs` unit tests add `number`, tuple and struct variants,
the enum reader and the decode message.

The 26 survivors:

- `geometry.rs:83:24`, `:88` (three), `:89` (nine) and `:90:31`. Every page-space matrix is
  `[1, 0, 0, -1, -left, top]`, so `b` and `c` are 0 and `det` is -1. Each mutant then computes the
  same number, or the same zero with the other sign, and the harness compares `-0` equal to `0`
  as the TypeScript's JSON does.
- `js.rs:57`, `>` to `>=` in `non_finite`. It only sees infinities and NaN, never 0.
- `js.rs:268` and `:286`, `skip_field` replaced with `Ok(())`. That is what `serde_json`'s value
  serializer does for it.
- `js.rs:421`, the three `size_hint` replacements. Serde uses the hint only to size an allocation.
- `kernel.rs:420`, the second `||` to `&&` in `rekey_session`. Every id in `documents` or
  `pending` also has a session (a session is set before its slot is published and deleted after
  it is unpublished), so `sessions.has` already answers.
- `kernel.rs:500`, `unwrap_or(-1)` to `unwrap_or(1)`. The active id is always in `order`, so the
  fallback is never taken.
- `lanes.rs:40`, the `$undefined` guard replaced with `false`. `cancel` turns an undefined reason
  into `'cancelled'` and arrays now print it as nothing, so `js_string` never sees one at the top.
- `rt.rs:480`, `Promise::ignore` replaced with `()`. The runtime has no unhandled-rejection
  report, so the task that awaits and discards does nothing anyone can see.
- `store.rs:90`, both mutants of `walking - 1`. The counter then never returns to 0 and tombstones
  are never compacted. A walk skips tombstones either way, so only memory differs.
- `token.rs:49`, `Hash` replaced with `()`. Every token hashes alike, but equality still tells them
  apart, so sets and maps hold the same entries.
