# epdf-kernel

`packages/core/main` in Rust: the kernel, its store, the plugin plan, resource scopes, the document
lifecycle and the v3 controller contract. All 91 cases in `vectors/kernel/lifecycle.json` and all
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

Mutation testing has not been rerun for this port.
