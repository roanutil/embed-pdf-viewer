# epdf-plugin-link

`packages/plugin/link` in Rust, ported to the current TypeScript (Task 13.21): `create()` builds the
controller, the stand-alone source is a kernel page mirror, `connect` registers the `link-nav`
behavior with the annotation plugin, and activation resolves to a page-space destination that the
stage reveals. All 37 cases replay (`cargo test -p epdf-plugin-link`), with no member-order
allowance:

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/link.json` | scenario | 29 | the page-mirror source (stable `obj:n` and `base:page:n` ids, flat `hidden`/`noView`, grouped replies, refused reads and retries, joined reads, refetch on confirmed changes, page drops), the annotation-backed source, `link-nav`, activation with and without a stage and through the action engine, hit testing, labels and `resolve`, `listAllLinks`, `onLoaded` |
| `vectors/plugins/link-functions.json` | function | 8 | `resolveLinkTarget` |

The scenarios run the real interaction, annotation, actions and stage adapters beside this one, in
the combinations a deployment ships.

`tests/behavior.rs` covers what the vectors don't print: a caller's cancel rejects that caller while
the shared read lands for the others, a signal already aborted still lets the read fill the cache,
`listAllLinks` cancelled midway keeps what landed, a close drops a read in flight quietly, `not-found`
for a ref or an index, hidden and grouped links, destination activation with and without a stage,
the website opener, and the turns a load takes, measured on the TypeScript.

## The public API

`link_plugin()` builds the plugin, `LINK` is its token and `LinkCapability` its capability, which is
also `LinkHostCapability`. The members are the TypeScript's in snake case: `list_links`, `get_link`,
`get_link_at`, `list_all_links`, `ensure_loaded`, `is_loaded`, `get_status`, `resolve`, `activate`,
`activate_at`, `get_label`, `on_activated`, `on_loaded`, `register_uri_opener` and
`is_navigation_engaged`. `resolve_link_target` is the shared pure rule, `links_of` the stand-alone
source and `pages_changed_by` the events that refetch a loaded page.

A link, its target, its destination and its `/A` tree are the engine's records as JSON (`Value`), as
the annotation plugin carries them since Task 13.17. A page argument is `PageTarget`, a `PageRef` or a
display index, as `PageRef | number`. `list_links` returns the page's list as an `Rc<Value>`, the
mirror's own while the page is unchanged. `LinkActivateContext` keeps each member as present or
absent, because activation spreads it over the link's own `activate` and `ref` and a present member
wins whatever its value; its `stage` member is the view the link was followed in.

## Migrating from the pre-13.21 API

For the viewer host and `epdf-web` (Task 13.33).

| Old | New |
|---|---|
| `link_plugin(LinkConfig { host: LinkHost { console_error } })` | `link_plugin()`; a throwing listener goes to the kernel's report |
| typed `Link` (`LinkNavItem`), `PdfLinkTarget`, `PdfDestination`, `HoverFlags` | `Value` records |
| `LinkResolution::Reveal { page, page_index, options }` | `LinkResolution::Destination`; the reveal happens in `activate` through `go_to_destination` |
| `resolve` and `activate` returning `Result` | infallible: page-space destinations need no inversion |
| `LinkInput` | the input as `Value`, a link when it has `target` and `bounds` |
| `LinkState`, `LinkAction`, `link_reducer`, `initial_link_state`, `load_links_page`, `project_links` | the kernel page mirror; `links_of` |
| `LinkActivatedEvent.origin` | removed with the TypeScript's |
| `ensure_loaded(&PageRef)`, `list_all_links()` | `ensure_loaded(page, OperationOptions)`, `list_all_links(OperationOptions)` |
| `get_input_label` | `get_label`, which takes either |
| ffi `PdfLinkTarget`, `PdfDestination` members, `LinkHandle::with_report` | `target_json`, `destination_json`; activation no longer fails, so there is no report |
| ffi `all_links` skipping a link with no page | an `invalid-input` error |

`src/ffi.rs` and `tests/ffi.rs` were migrated by hand and are not compiled until `epdf-ffi` builds
(Task 13.33). The viewer host's call sites move with them: `crates/epdf-viewer-host/src/session.rs:106`
(`link_plugin(Default::default())`) and `crates/epdf-viewer-host/src/lib.rs:452`
(`LinkHandle::with_report`).

## Where the port differs

Kept Rust-only divergences, none reached by a vector:

- A `goto` destination the stage's `PageDestination` cannot decode gets a bare reveal of its page,
  which is what `goToDestination` does for an unknown `kind` (`revealOfDestination` has no spot for
  it). For a known kind with a member the stage cannot read, the TypeScript places the camera from
  that member; Rust reveals the page bare and sends the decode error to the kernel report
  (`an_undecodable_destination_still_reveals_its_page`).
- A context `page` that is not a `PageRef` is dropped from the action source, which the actions
  plugin types; the TypeScript passes it through.
- `get_label` of a `uri` or `name` that is not a string answers its JSON text, where the TypeScript
  returns the value.
- With the annotation plugin installed, `list_links` wraps the plugin's cached list in a fresh `Rc`,
  so identity is per call there.
- `links_of` gives an `objectNumber` ref with no number the id `undefined`, where `annotationKey`
  builds `"obj:undefined"`, and a `baseIndex` ref with no page the same, where the TypeScript throws.
- A `null` list snapshot fails `annotations is not iterable`, where the TypeScript throws
  `Cannot read properties of null (reading 'annotations')`.
- `get_link_at` treats a link with no `bounds` as a miss, where the TypeScript throws reading it.
- `resolve` reads a destination with no `page` as a page the document doesn't have, where
  `pageRefsEqual` would throw reading off `undefined`.
- The harness's scripted `registerUriOpener` reads a throwing opener as a refusal, since `UriOpener`
  answers a `bool`; the TypeScript's throw leaves `activate`. No vector registers one.

`loads_settle_on_the_typescripts_turns` pins the turns a load takes, measured on the TypeScript,
including the three of the `.finally` that `page-mirror.ts:194` chains onto each load (kernel
`6e678f93b`).
