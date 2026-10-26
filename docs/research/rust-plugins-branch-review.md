# Review: research/multi-platform-rust-core-impl-plugins

Scope: `git diff research/multi-platform-rust-core-impl...research/multi-platform-rust-core-impl-plugins` (merge base c8085bd3c, tip 81899a80a). Every path below is relative to the repo root, and line numbers are at the plugins tip as checked out in `.claude/worktrees/rust-plugins`.

Method: 13 parallel reviewers, one per area, each told to read every non-generated changed file in its area and compare against the TypeScript reference. I re-read four findings at the cited lines myself: H3, H4, M5 and the epdf-web harness dependency (L-W8). Two reviewers reproduced their crashes in scratch builds (H1, H2). The kernel reviewer measured microtask turn counts with a Node script to back M1 to M3. Only the build/CI reviewer ran cargo or node to test a claim (`cargo test -p epdf-web` on the host, `node --test test/config.test.mjs`, `cargo metadata --locked`). Everything else is from reading code paths, and the confidence on each finding says which.

Totals: 0 Critical, 4 High, 34 Medium, 89 Low.

Confidence key: "reproduced" means a reviewer ran it; "verified" means the code path was traced end to end; "plausible" means the path exists but depends on an input or timing that was not confirmed.

Some findings are behavior the TypeScript reference shares ("shared with TS"). They are still listed because Rust fails differently: a TS TypeError can be caught, while a Rust panic aborts a native host (edition 2024 `extern "C"`, no catch_unwind on napi exports per Cargo.toml:53-58) or traps the whole wasm instance.

## Status, 2026-10-13

All four Highs and 29 of the 34 Mediums are fixed on `research/multi-platform-rust-core-impl-plugins` (tip `f4569afc1`), one commit per finding, each with a test. The Lows are not addressed yet. On that tip, `cargo test --workspace` passes 1,028 of 1,028, `core-wasm` 72 of 72 and its gc suite 13 of 13, and `vectors:check` and `coverage:check` are current.

| Finding | Commit | Note |
| --- | --- | --- |
| H1 | `24e77c1a3` | BER depth capped at 64 |
| H2 | `7d0c4aa1d` | RSA-PSS salt length range-checked |
| H3 | `046d47578` | Also changes TS (`packages/core/annotation`), by choice |
| H4 | `d365c22a7` | A real monotonic clock for the script budget |
| M1 | `0f74cd1f5` | Open settled 3 turns early, not 4; unlock was already right. Adds an opt-in turn meter to both scenario runners and `kernel/turns.json` |
| M2 | `101997143` | |
| M3 | `545ef5184` | Changes await timing in the kernel and in five plugins' engine helpers |
| M4 | `19a614a9a` | |
| M5 | `8aad6ae39` | |
| M6 | `d15ef217c` | |
| M7 | skipped | The Swift package can't be built here: the `EmbedPDFFFI.xcframework` iOS builds are missing |
| M8 | `f87b4ab00` | |
| M9 | `4e36f5db1` | |
| M10 | `5f4777ef7` | `scopeOf` still throws after destroy: the shim has no list of the plan's tokens |
| M11 | `19835b20d` | |
| M12 | `4572f45bc` | |
| M13 | `22ca510e6`, in part | A realm built after dispose is released. Rebuilding a faulted realm inside one transaction matches TS (`host.ts`) and stays |
| M14 | `0bef57fe5` | Also changes TS (`plugin-actions`), by choice |
| M15 | `7d53dae0c` | Rust-only divergence: TS has the same gap |
| M16 | skipped | Shared with TS (`hydration.ts:59-64`); fixing it changes observable behavior, so TS and the vectors go first |
| M17 | `965d22aa0` | |
| M18 | `99a15c59b` | 20,000 annotations hydrate and remove in 0.15 s, from 2.2 s |
| M19 | `8e3cecfb1` | 10,000 annotations thread in 0.4 s, from 1.8 s |
| M20 | `26e0aa3b0` | Cloudy and squiggly paths stop at 1,000,000 curls (`MAX_CLOUDY_CURLS`) |
| M21 | `7ee92d1ed` | `get_snapshot` now returns `Option<Rc<Value>>` |
| M22 | `5d05a8e45` | Also a second cubic path in `read_model` the review missed; select-all on 1,000 pages, 277 s to 2.3 s in debug. Since Task 13.15 (Ruling13.15) the guard counts registry decodes, at most 2 across a 400-page select-all and read, with a 10 s hang ceiling (`crates/plugins/selection/tests/scale.rs`) |
| M23 | `e8205de75` | Rust-only divergence: TS has the same race |
| M24 | `81c9e0805` | |
| M25 | `f0b535982` | Rust-only divergence: TS strands the slot too |
| M26 | `fa2ba2f00` | `conform_viewport` answers a NaN width where TS has none |
| M27 | `4965e0519` | Seven stage verbs now return `Result`. A NaN column count lays out an empty scene (Rust-only divergence), where TS throws on every scene build |
| M28 | `5b52413b1` | |
| M29 | `203a5f5e4`, `c47e310f2` | |
| M30 | `43d450a2b` | `vectors:check` already runs in `plugin-rust.yml`, so CI enforces it with no workflow change |
| M31 | not taken | CI work was out of scope |
| M32 | `33ca050b8` | |
| M33 | `e477f965c` | |
| M34 | `d34708034` | `@embedpdf/engine-core` may need a changeset too; not checked |

"Rust-only divergence" means Rust no longer has a bug TS still has, on a path no conformance vector covers.

Lows, as of 2026-10-12 (plugins tip `f2e9b555b`): 63 fixed (one in part), 26 not. On that tip, `cargo test --workspace` passes 1,077 (1 ignored: the opt-in no-harness build check), `core-wasm` 78 of 78 and its gc suite 13 of 13, the `epdf-web` wasm tests 38 of 38, and `vectors:check` and `coverage:check` are current.

| Area | Fixed | Not fixed |
| --- | --- | --- |
| Kernel | K1 `b45d256f8`, K2 `6567eb1eb`, K3 `1d7d1d547`, K4 `0371bee7c`, K5 `884d18d16`, K6 `5a81da047` `b6cb471b5`, K7 `a58afe87d`, K8 `844251966`, K9 `e4e5148d1` | |
| Wasm boundary | W1 `fa771b30d`, W2 `800a4cb77`, W4 `1ce6fabf1`, W5 `38934ad99`, W6 `269207f05`, W7 `fe68a8ca7` | W3 left: immutable snapshots would cost eager copies in every flow. W8 by design (evaluation crate) |
| FFI, host, Swift | F4 `b6e69bfab`, F5 `9b1a52698`, F10 `375aa71f7` | F1, F2, F3, F6, F8, F9 blocked: Swift can't be built here. F7 needs a Swift change |
| Actions, form, acrojs | A1 `656c481d5`, A4 `40e81c5a6` (README note), A5 `ec8ee2ba4`, A6 `c601d69ef` | A2, A3, A7 shared with TS |
| Annotation | N1 `438dc9706`, N3 `4619c7df5`, N5 `357358e37` (a port of V8's sort), N6 `343fc063e`, N7 `aa913bc4e`, N10 `cc804f408`, N11 `eee8ee38a`, N12 `b85c0a9cd` | N2, N4, N9 shared with TS. N8 unreachable after N6 |
| Selection, search, link | S1 `f02a66ee3`, S2 `3e08ee2d7`, S3 `d383ee3a7`, S5 `6a6332e26`, S7 `74045ce86`, S8 `c0ef4b3d7`, S9 `66719af32` | S4, S6 shared with TS |
| Render and stage | R1 `4bb490cb5`, R2 `daa86f9a9`, R3 `4ed4566cd`, R4 `093e457e9`, R5 `1bd756fb3`, R7 `0695bc428` (Rust-only guard) | R6 no evidence of a difference |
| Signature, stamp, redaction | G1 `873ae8fb1`, G4 `c45f3d808` | G2 shared with TS. G3 wrong: `bounds` was already optional (`44385e5fb`) |
| Commands, i18n, shell, ui-core | U1 `61d325d25`, U2 `013f02bf9`, U4 `3045b8144` in part, U7 `0a45ee495`, U8 `89fc48978` | U3, U5 shared with TS. U6 refactor only |
| Conformance | C1 `1d25e8e90`, C2 `b4997ffeb` (tooling check, no CI change), C3 `2a1639501`, C4 `4c116cd6a`, C5 `4b23b458b`, C6 `cf73829d9`, C7 `347b49e86`, C8 `e42653389`, C9 `5b713eb40` | |
| Build, docs, perf | B1 `7c0446edf`, B2 `784da6599`, B3 hostnames `4c2c8527b`, B6 `61339677b` | B3 root docs: moved in `91344f785`, moved back in `f2e9b555b` by choice. B4 CI. B5 documented choice |

New Rust-only divergences among the Lows: R5 (a scene with an unreadable page row lays out empty, where TS throws on every build, as M27), R7 (a zero tile span yields no tiles, where TS loops), and part of N6 (a ref of unknown kind fails, where TS keys it `"undefined"`). U4 is in part: a remover no longer holds its own definition, but a closure that captures the strong commands capability still leaks; `CommandsCapability::downgrade` gives closures a weak handle. C1's member-order comparison first found 896 cases in 59 vector files where Rust wrote object members in another order than TS.

Another Rust-only divergence, from Task 13.7 (Ruling13.7): a kind name that `Object.prototype` holds. TS `kindNamed` reads `KINDS[name]` (`packages/core/annotation/src/kinds/index.ts:88`), so `constructor`, `toString`, `__proto__` and `hasOwnProperty` find an inherited value instead of a kind, and later reads of it throw a `TypeError`. Rust `kind_named` (`crates/plugins/annotation-core/src/kinds.rs:980`) reads them as any other unknown name: a kind that is only shown. The core is kept that way. The harness keeps printing the TS answers for those names, in `kindNamed`, `propertiesOfKind` and `kindTakesLink` (`crates/plugins/annotation-core/src/harness.rs:949`, `:956`, `:963`) and `defaultsFor` (`:347`), so 13 cases still compare against TS rather than the core: the four names under each of `kindNamed/`, `propertiesOfKind/` and `kindTakesLink/` in `core/annotation-kinds.json`, and `defaultsFor/prototype-key` in `core/annotation-props.json`. The core side is pinned by `a_name_object_prototype_has_is_a_kind_only_shown` in `crates/plugins/annotation-core/tests/behavior.rs`. `epdf-fonts` makes the same choice for face names (`crates/plugins/fonts/README.md:50-53`): `FACES['toString']` finds a function in TS, and Rust treats `toString`, `__proto__` and `constructor` as unknown names.

Task 13.7 also keeps three refusals no vector reaches, listed in `crates/plugins/annotation-core/README.md`. A `setSnap` patch the typed settings can't hold leaves the model unchanged, where TS spreads it in. A malformed `/Measure` dictionary is read loosely where TS throws or prints `"undefined"`. A cloud outline over 1,000,000 points is empty, where TS allocates until memory runs out (Ruling13.7, M20). As of that task the member-order allow-list holds 334 paths in 155 cases of 19 files.

Task 13.8 keeps M13 against a vector that now observes it (USER DECISIONS 2026-10-23). In `core/acrojs-host.json` case `dispose-pending-factory`, a run admitted before `dispose()` builds a second realm. The TypeScript releases that realm only at the repeated `dispose()` (step 4), because `packages/core/acrojs/src/host.ts:130` builds with no disposed check, `:216` refuses only new transactions, and `:231-238` releases only the realm held at each call. Rust releases it when the last admitted transaction settles (step 3), as M13 (`22ca510e6`) does, pinned by `a_realm_built_after_dispose_is_released_when_the_queue_drains` in `crates/plugins/acrojs/src/host.rs`. The harness compares that case through `crates/plugin-conformance/outcome-exceptions.json`, which names the file, the case and the one callback (`sandbox.dispose [1]`) with its Rust and TypeScript steps. Every other part of the trace still compares exactly. An entry fails the replay when the trace no longer differs, when the callback isn't logged exactly once where listed, or when its case doesn't run (`crates/plugin-conformance/src/exceptions.rs`). It holds one entry.

Task 13.9 records a CMS-parse divergence in `epdf-signature-core` it does not fix (code fix carried to Task 14). The Rust reader joins a constructed OCTET STRING wherever a value is read with `Node::octets()`: the SignerInfo signature (`crates/plugins/signature-core/src/parse.rs:188`), the message-digest attribute (`:400`) and the ESS certHash (`:434`). pkijs's schema wants a primitive OCTET STRING there. `der::from_ber` also takes two encodings asn1js 3.0.10 refuses as not valid BER: a constructed primitive-only type and a constructed OCTET STRING with a member of another type. The 13.9 re-review re-encoded the `rsaCades` fixture four ways: the signature as a constructed OCTET STRING of two pieces, the same with a nested constructed piece, the same with an INTEGER piece, and the `020101` versions as constructed INTEGERs. All four are `invalid` in TS `verifyCmsSignature` (a schema error, or "CMS is not valid BER") and `Valid` in Rust. Nobody without the key can forge one: only unsigned encoding changes, the signed attributes still verify under the carried certificate, and the message digest still binds the `/ByteRange`. The visible effect is a verdict that differs by platform for a re-encoded genuine signature. No vector exercises it. The TSTInfo reader for document timestamps already applies asn1js's rules (`timestamp.rs`). `crates/plugins/signature-core/README.md` states the behaviour.

Task 13.10 keeps three Rust-only divergences in `epdf-ui-core`, each listed in `crates/plugins/ui-core/README.md`:

- Layout, menu, panel and key-name lookups read own members only. TS finds `Object.prototype` members for `constructor` and `__proto__`, so `add(item, { to: 'constructor' })` edits a `constructor` bar and `formatShortcut('Ctrl+constructor')` prints `Ctrl+` (`⌃undefined` on a Mac, `packages/core/ui/src/shortcuts.ts:139`). Rust warns `no bar or menu "constructor"` and prints `Ctrl+Constructor` (`⌃Constructor` on a Mac), as Ruling13.7 chose for kind names (`an_object_prototype_name_is_no_bar_menu_or_key_name` in `crates/plugins/ui-core/tests/behavior.rs`).
- A `null` bar or panel, or layout data that is `null`, is read as empty where TS throws a `TypeError`: `validateLayout` beside an anchored edge bar (`packages/core/ui/src/layout-validate.ts:135`), `replace` and `removePanel` (`entriesOf(null)`), `panels()` with an object `present` or a `width` (`packages/core/ui/src/layout.ts:664`), and any edit of `createLayout(null)` (`a_null_bar_or_panel_is_skipped_where_the_typescript_throws`).
- A bars, menus or panels record that is a string spreads into UTF-16 unit keys in TS; Rust reads each half of a surrogate pair as U+FFFD (`spreads_and_reads_members_as_javascript_does` in `crates/plugins/ui-core/src/value.rs`).

Task 13.11 keeps one new Rust-only divergence in `epdf-plugin-interaction`: settings tools compare by identity. The kernel keeps a registration's settings as JSON, and a tool can carry callbacks, so `encode` (`crates/plugins/interaction/src/settings.rs:58-68`) stores each typed tool as a handle into a table the registration owns. Two distinct tools with equal data and no callbacks are therefore a change. TS `changedKeys` uses `sameValue` (`packages/core/main/src/settings.ts:187-222`), which deep-compares a plain tool given without `enables` and calls it no change, while a tool with an `enables` set compares by identity in both. Rust can't converge because `Tool.enables` can't tell a tool given without `enables` from one given an empty set. Where they differ, Rust fires `onSettingsChanged`, swaps the setting's tools, and the swap's removers drop those ids' `setToolCursor` entries, where TS does nothing. No vector reaches it; `distinct_equal_settings_tools_count_as_a_change` in `crates/plugins/interaction/tests/api.rs` pins it and `crates/plugins/interaction/README.md` states it. The same task removed the six `plugins/interaction.json` member-order allowances, which the re-ported adapter no longer needs: the allow-list now holds 327 paths in 148 cases of 17 files.

Task 13.12 keeps M14 and records one new Rust-only divergence in `epdf-plugin-actions`. M14 stays because upstream does not subsume it: upstream's print door (`packages/plugin/actions/src/lifecycle/document-events.ts`, `firePrintThroughAdapter`) refuses a non-user print only when the kernel has a printer, and the branch's kept TS change (`5462de7d9`, `253bca66b`) reads the `print` row below that branch for a script print on the adapter path. Rust has no kernel printer yet (Task 13.33), so every Rust script print takes the adapter path, where the row decides; `m14_a_lifecycle_script_print_under_the_default_block_never_reaches_the_adapter` and `m14_a_user_script_print_follows_the_live_print_row` in `crates/plugins/actions/tests/behavior.rs` pin it on the real executor, and `actions-script.json` `script-print-obeys-the-print-policy` replays it. The form pipeline's script print (`scripting/surface.ts`) still skips the row in both languages, pinned by `the_form_pipeline_print_still_skips_the_print_row_as_the_typescript_does`; it stays the Task 14 carry. The new divergence: with `javascript.enabled` on and no sandbox factory, the TypeScript runs scripts in a lazily loaded QuickJS, and Rust, which has no JavaScript engine, gives the document the same realm but rejects every sandbox build with `NO_DEFAULT_SANDBOX`, so each javascript node fails (`scripting_on_without_a_factory_has_a_realm_whose_runs_fail`). No vector runs a script in that configuration (`no-sandbox-host` closes the document first). L-A4 is converged: `canDispatch` reads only the `triggers` setting in both. M15 (URI scheme check) and A1 (a dropped detached realm releases its sandbox, `a_detached_realm_dropped_without_dispose_releases_its_sandbox`) stay. The member-order allow-list keeps its one `plugins/actions-submit.json` entry, which the replay still needs.

Task 13.14 keeps M27 and R5 in `epdf-plugin-stage`, and the paged-empty camera refusal stays an explicit TypeScript limitation. M27 and R5 now live in one place, `layout_for` in `crates/plugins/stage/src/services/scene.rs`: the registry rows stop at the first unreadable one (`map_while`), and a layout error from stage-core (a NaN column count, or a grouping naming a missing page) yields an empty scene where TS throws on every build. No vector records either throw; `a_nan_column_count_empties_the_grid_instead_of_throwing` and `an_unreadable_page_row_keeps_the_others_at_their_indexes` in `crates/plugins/stage/tests/api.rs` pin them. The paged-empty refusal converges on the outcome: in paged flow over no pages, TS dereferences the missing slice item (`packages/plugin/stage/src/services/scene.ts` `itemRect` under `stayBounds`), and Rust's `stay_bounds` checks for it and returns the same `TypeError` (`Cannot read properties of undefined (reading 'x')`) after the same earlier writes, with no panic; `a-document-with-no-pages` replays its seven refusals in steps 36 to 43, and `paged_flow_over_no_pages_refuses_camera_writes_as_the_typescript_does` pins it. M26 is render's (13.16). One new Rust-only divergence: `patchSettings` lands a key that is not `Object.is` the current value, and Rust has no object identity for settings, so an object-valued setting (page frame, alignment, zoom spec, `{px}` gap) always counts as new (`patch_changes` in `crates/plugins/stage/src/settings.rs`). The TypeScript writes nothing when handed back the very object the state holds (a `getViewpoint().zoom` passed to `goToPage`, or an object setting holding NaN, which `eqSetting` never equates, re-patched with the same object); Rust records a state change with no settings event. No vector reaches it. A second Rust-only divergence: a page transform that is not invertible (a view scale whose square underflows, e.g. device ratio 1e160 over an unbounded zoom of 1e-170) makes TS throw `Mat2D.invert: non-invertible matrix` from the page reads; Rust leaves the page out of `list_visible_pages` and answers `None` from `get_page_frame` (`with_transform` in `crates/plugins/stage/src/read/pages.rs`, pinned by `a_non_invertible_page_transform_leaves_the_page_out`). M27's `Result` half is pinned again by `non_finite_camera_input_on_a_grid_is_an_error` and `a_nan_zoom_on_an_auto_wrapped_grid_does_not_panic`. Camera and viewport identity, which the events compare, are carried exactly (`CameraObject`, `viewport_writes` in `crates/plugins/stage/src/model.rs`). The working set reaches the engine through the new `DocumentHandle::set_working_set` (kernel default no-op, guarded delegation; the conformance fake ignores it as the TS fake does); forwarding it in the native and web handles is 13.33's. The task removed every `plugins/stage-*` member-order allowance (189 paths in 82 cases of four files); the allow-list now holds 138 paths in 66 cases of 13 files.

Task 13.15 keeps M22, M23 and S1 to S3, S8 and S9 in `epdf-plugin-selection` and adds one Rust-only divergence. A `page.text.layout` answer that is neither a `$textLayout` marker nor a page-space geometry snapshot reads as a loaded page with no text (`hydrate` in `crates/plugins/selection/src/controller.rs`), where the TypeScript keeps the answer and throws when a layout method is called on it (`packages/plugin/selection/src/controller.ts:105`). No vector answers one. M23 stays as it was: a content change during `readText` fails the read with `conflict` (`tests/stale_text.rs`), now keyed to the pages `contentChangedPagesOf` names, since upstream invalidates only those. S1 now covers a row whose `ref` does not decode, which `pageAtIndex` skips where TypeScript hands the raw value on (`tests/malformed_registry.rs`). The crate's text layout stays a pure copy of the engine's (Ruling13.15), cross-checked on native hosts by `tests/engine_cross_check.rs`. The copies differ in two places, both on input PDFium never produces: indices are `i64` here, where a caller can name a range before the page, and the S2 guard reads runs that do not tile without indexing past the glyphs, where the engine's copy panics (the two vector layouts `rotated-all-empty` and `char-start-past-the-glyphs`). The task removed both `plugins/selection.json` member-order allowances; the allow-list now holds 136 paths in 64 cases of 12 files.

Task 13.16 keeps M25, M26 and R7 in `epdf-plugin-render` and adds two Rust-only divergences; M23 is selection's and has no render code. M25 changes shape: the TypeScript's `fetchTile` is now `async`, so the plugin's own fetch never throws, and the guard is for a Rust embedder of `TileManager` whose `fetch_tile` answers `Err` at once. That tile fails and frees its slot where a synchronous throw would strand both (`a_synchronous_tile_refusal_frees_its_slot` in `crates/plugins/render/tests/api.rs`). M26 (an empty width ladder or tile pyramid degrades instead of panicking) is replayed by `empty-width-ladder`, `empty-tile-ladder` and `empty-lattice-tile-scales`; R7 is pinned by `a_zero_tile_span_yields_no_tiles_instead_of_looping`. The new divergences: an event missing or mistyping a member `pixelChangeOf` reads names no page for it, where the TypeScript throws a `TypeError` in its listener or keys the ledger with the value as given (`crates/plugins/render/src/invalidation.rs`: an annotation or flatten fact with no `page`, a result with no `page`, a non-array `widgets`, `results` or `meta.changedWidgets`, and an `objectNumber` that is not a non-negative integer within `u32`); and a settings value the port cannot decode (written past the typed API through `Kernel::settings_of`) reads as `render_defaults()` and warns once through `RenderHost::warn`, at all three sites that decode it in `crates/plugins/render/src/controller.rs`: `Core::resolved`, `Core::get_settings` and the `on_settings_changed` payload (`a_malformed_setting_reads_as_the_defaults_and_warns_once`). No vector reaches either. Ruling13.16 adds `CallFacts` and `DocumentHandle::call_with` to the kernel (default `call`, guarded delegation, the fake silent) so render asks with `{priority: 'high', view}` as `controller.ts` does; web and native forwarding is 13.33's. The page-layer rule (`resolvePageLayers`) is a pure copy in `crates/plugins/render/src/model.rs`, since `epdf-engine` links PDFium and render builds for wasm32. All six render vector files replay with no member-order allowances.

Task 13.17 re-ports `epdf-plugin-annotation` onto the current kernel, keeps N6 and M19. H3 is closed by the port's shape, as the earlier ruling said upstream closes it: records keep their object-number key from the first frame, and a record that leaves the view leaves the session in the same notification (`crates/plugins/annotation/src/services/store.rs`, after `packages/plugin/annotation/src/services/store.ts`); the `reconcile/h3-*` cases in `annotation-reconciliation.json` replay it and `h3_a_record_deleted_mid_drag_leaves_the_session_before_the_next_sample` in `crates/plugins/annotation/tests/behavior.rs` pins that the forget runs before the next pointer sample. N6 stays (`dto_key` in `crates/plugins/annotation/src/sync/records.rs`, `n6_a_record_whose_ref_has_no_key_is_refused_and_the_others_stay`). M15 has no annotation code; it stays with actions. M16 is superseded: hydration is the kernel's records mirror, whose `refresh` clears `running` on every path (`crates/epdf-kernel/src/mirror.rs`, as `packages/core/main/src/mirror.ts` `refresh`), and the plugin's hydration is gone. Fix round 1 (Ruling13.17 onLost/fonts): lost object numbers are no longer a divergence. The kernel's `DocumentHandle::on_lost_object_numbers` (`f48e45efa`, silent by default as the TypeScript fake is) carries `objectNumbers.onLost`, and the plugin subscribes the first time it takes numbers (`a_lost_object_number_leaves_the_session`). The engine font list stays a Rust-only difference, owned by Task 13.33: the plugin's font lookup is always empty, where the TypeScript reads a local engine's registered fonts (`packages/plugin/annotation/src/services/index.ts:71`); the cloud path is the same in both (an empty list), and no vector registers a font. A new Rust-only difference: an icon placement `iconPlacement` refuses (an attachment with no name) is logged as `[annotation] icon placement failed:` and the click counts as handled (`write/icons.rs`), where the TypeScript throws inside the picker's `.then` as an unhandled rejection. M19 is kept with a deterministic guard (`m19_the_threads_index_is_built_once_per_change`, `Rc::ptr_eq` on two reads) and a 10,000-annotation build under a 10 s hang ceiling, per Ruling13.15 M22. L-N3 moved into the kernel `cancellable` and is pinned there (`a_cancellable_call_releases_its_signal_once_it_succeeds`). L-N7 converged on the TypeScript: `QuarterTurn` is gone and `selection_rotate_by` takes degrees as `rotateBy` does, so its test is obsolete. M16's sequence is pinned by `m16_a_read_grant_after_a_forbidden_load_is_seen_on_refresh`. Two `annotation-hydration.json` cases (`mixed-type-cursors-compare-as-javascript`, `string-cursors-compare-as-strings`) exposed a kernel defect, repaired under Ruling13.17 cursor in `99cb013fc`: the mirror read the load cursor and each event's `serverId` as `u64`, where the TypeScript compares them with JavaScript's `<=` (`packages/core/main/src/mirror.ts:416-419`). `Loaded.cursor` is now a `Value` and `js_less_or_equal` in `crates/epdf-kernel/src/mirror.rs` compares as JavaScript does; all eleven annotation files replay. The member-order allow-list loses its `annotation-api.json`, `annotation-pure.json` and `annotation-tools.json` entries, which the replay no longer needs.

Task 13.18 re-ports `epdf-plugin-form` onto the current kernel and keeps H4 and A5. H4: the K/V/C/F transaction's time budget runs on `rt::monotonic_now` (`crates/plugins/form/src/scripting/controller.rs`), and `tests/budget.rs` gets two 30 ms runs out of 50 ms while the virtual clock stands still. A5: `importValues` and `exportValues` order names as a plain object does, replayed by `form-fill.json` `values-keyed-by-integer-like-names`. A6 is converged: placement names fields over a `HashSet` (`nextFieldName`). M21 is kept (Ruling13.18 tests/M21). The round-0 port deep-copied the form on the plugin side several times per fold or prediction (`upsert_fields`, `with_snapshot_member` and `to_mirror` in `model.rs`); the fix round keeps `fields` and `widgets` as `Rc` items in `model::Snapshot`, so a change replaces one member and shares the rest, and the `fields` mirror holds a token naming the index, so the kernel's copy on write is a few bytes. Fix round 2 (Ruling13.18 mirror cell): the store cell `form::a/mirror/fields` that `signature.json`, `stamp-api.json` and `stamp-placement.json` dump shows the whole `FieldIndex` again, through the kernel's dump-time `MirrorSpec::to_json` (`640a17350`), pinned by `the_fields_mirror_cell_dumps_the_whole_field_index` against `signature.json` `reads-over-the-snapshot`; a token the cache no longer holds logs `[form] the fields mirror named a form it no longer keeps:` and shows the confirmed form. `crates/plugins/form/tests/sharing.rs` holds the deterministic copy count (`snapshot_copies`) at none across folds, predictions, fills and reads under a hang ceiling, per Ruling13.15 M22, and restores `the_fill_projection_is_reference_stable`. `listWidgets` is memoized as `memoByKey` does. A new divergence, carried to Task 14: the TypeScript's other identity memos (`getValue`, `getWidgets`, `exportValues`, `validate`, `getSelectedField`) build a new value per call in Rust; the values match, a reader comparing by reference re-renders more often. Four further Rust-only console lines and errors: `[form] the painter could not read a widget:` (`services/looks.rs`, the TypeScript has no decode step), `[form] surfacing a fill failure threw:` (`write/fill.rs` `fill_soon`, an unhandled rejection in the TypeScript), a lend's `renderAppearances` rejecting on a picture that does not serialise (`write/lending.rs`), and a `TypeError` from the activation verbs for a widget ref with no `page`, which the TypeScript passes on as `undefined` and `epdf-plugin-actions` types as a `PageRef` (`write/activation.rs` `page_of_ref`). Three TypeScript limitations stay as the TypeScript has them, each for the Task 14 decision: the palette's form-only early return leaves an engine refusal of the create unobserved (an unhandled rejection in TypeScript, no channel in Rust; `a_form_only_placement_refusal_ends_before_it_is_awaited`), the form pipeline's script print skips the `print` row (`packages/plugin/actions/src/scripting/surface.ts:120`), and default QuickJS execution has no Rust counterpart (`NO_DEFAULT_SANDBOX`). `flags.raw` is gone with upstream, and nothing stands in for `DoNotSpellCheck` or `Sort`. The kernel `SerialQueue` lacks the TypeScript's `signal` option and `idle()`; `crates/plugins/form/src/services/queue.rs` adds both over it without changing its order or timing; `crates/plugins/actions/src/dispatch/dispatcher.rs:87-105` carries the same `{signal}` wrapper, so the Task 14 kernel `SerialQueue` carry covers two plugins. The form now pulls `epdf-form-core` and `epdf-fonts` into `epdf-web`, so `epdf-fonts` keeps its programs, decoded at build time, in a table apart from the metrics (Ruling13.5(b)): a wasm32 probe that only reads metrics went from 461,992 to 34,554 bytes. All 221 cases replay. `form-design.json` `placement-refused-by-the-engine` passes since the kernel's `to_plugin_error` passes a `PermissionDenied`'s `details` through whole, as `packages/core/main/src/errors.ts:131` does (`1ff58faa4`, `526059c29`; `a_permission_refusal_keeps_its_details_whole`). The member-order allow-list and the outcome exceptions hold no entries for the six form files.

Task 13.19 re-ports `epdf-plugin-page-edit` onto the current kernel. No finding in this register names page-edit code; M29's example "page-edit 17/19" is out of date, since every case in `plugins/page-edit.json` now compares state. Four pre-13.19 behaviours are obsolete with the TypeScript that pinned them: the per-verb `SerialQueue` (edits are changes on `ctx.changes`), the ffi's `move_pages_to_end` index for `End` (`'end'` is a position the engine resolves, `controller.ts:69`), the `TypeError` for a fractional index position (index positions are gone), and a closed or unknown page reaching the engine before refusal (`refsOf` and the change queue refuse first). Kept: the `f64` rotation wrap with `Map` key grouping, and the Rust-only `$hostBytes` release when staging an insert fails. New Rust-only divergences, none reached by a vector and each listed in `crates/plugins/page-edit/README.md`: an `insertedPages` that is not an array resolves `[]`, `extract` rejects an answer that is not bytes, `can_edit` answers `false` without a handle where `ctx.allows` throws, and a position object with neither `after` nor `before` refuses `not-found` where TS throws a `TypeError`. All 19 cases replay since the generator writes non-finite step arguments as `$number` (`9584ec989`). The awaits take the TypeScript's turns, pinned by `await_first_verbs_stage_on_the_typescripts_turns` in `crates/plugins/page-edit/tests/behavior.rs`.

Task 13.20 re-ports `epdf-plugin-search` onto the current kernel. S7 is converged: `listPagesWithHits` now walks the document's page list (`model.ts:120`), so the order is document order and the ffi doc says so. The pre-13.20 last-page cache in `hits_from_slice` is kept in a stronger form, a pon-to-row map built once per page list (`Registry` in `crates/plugins/search/src/controller.rs`); no M finding named it, and no guard test covered it before or now. Obsolete with the TypeScript that pinned them: the plugin-side PDF-to-page quad flip (the engine answers page-space segments), `SearchMode` (now `snippets: boolean`), the reducer and its unknown-action case (named transitions), and the `operationId` on events. Kept Rust-only divergence, listed in `crates/plugins/search/README.md`: integer hit indices for `goToHit`. A lane's abort reason reaches the engine as its string through the kernel's `cancellable` since `4ad664efd` (Ruling13.20). `validateSearchQuery` and `validateSearchRegex`, which `contract.ts` re-exports from engine-core, are not re-exported: they live in `crates/epdf-engine/src/features/search/mod.rs`, which links PDFium. All 76 cases replay with the search entries removed from `member-order-allow.json`, and `scans_settle_on_the_typescripts_turns` in `crates/plugins/search/tests/behavior.rs` pins the turn counts.

Member order, as of 2026-10-13 (plugins tip `8cf572cf5`): the comparison is on by default, and every vector passes it. Real maps now keep insertion order (annotation `defaults` and `byId`, form `byKey`, stamp libraries and assets, the kernel's slices and documents), struct fields follow the TS literals, and where TS gives one type several orders by producer the order is set where it is serialized (cameras, rects, link items, number formats). The commits are `b5942dfc5`, `596d75f8a`, `be3ea170b`, `bb4e00760`, `58c4e4603`, `8fa954608`, `31e7b926d`, `8c75213f4`, `8f1565ff4`, `15e576f2b`, `60ae3d6e6`, `e94ef6188`, `d41ef4ee1`, `9f5fd252f` and `8cf572cf5`. What a typed Rust value can't follow, an object TS passes through from its caller or a type whose order depends on which function built it, is listed in `crates/plugin-conformance/member-order-allow.json`: 375 paths in 186 cases of 25 files, about half of them stage cameras. Two commits that tracked input order in public structs (acrojs `ScriptEventInput`, actions `ActionSubmitRequest`) were dropped in `306a293cd` to keep those types plain; their two fixture cases are on the allow-list.

Public signatures changed by the Low fixes: `deliver_remote_event` and `get_selection_props` return `Result`, `rotate_selection_by` takes `QuarterTurn`, and `commands::MenuTarget` is now ui-core's type.

## High

### H1. Nested CMS bytes overflow the stack and abort the process
- Where: `crates/plugins/signature-core/src/der.rs:148-247` (`read` recurses once per nesting level, no depth cap), plus the `raw: bytes[start..end].to_vec()` copies at :214 and :240.
- Reached from: `parse_cms_internal` (parse.rs:279), called by verdict.rs:85 for every signed field's `/Contents`. The signature plugin runs this automatically on connect when anything is signed (`crates/plugins/signature/src/controller.rs:121-138`), on every revalidate, and in the sign gate. trust.rs and cert.rs reuse the same reader.
- Scenario: a PDF whose `/Contents` is 50,000 nested `30 80` pairs (200 KB). Opening it gives "thread 'main' has overflowed its stack ... aborting". At depth 10,000 (40 KB) it survives but allocates about 200 MB of `raw` copies. A wasm 1 MB stack traps on a much smaller file. In TS, asn1js throws a catchable RangeError and the user sees a `malformed` verdict.
- Confidence: reproduced (release build, `verify_cms_signature`).
- Fix: cap depth (for example 64) and return `None`, which surfaces as `malformed`. Store `raw` as an offset range into a shared buffer.

### H2. An attacker-chosen RSA-PSS salt length panics inside `rsa`
- Where: `crates/plugins/signature-core/src/crypto.rs:99-102` (unchecked `acc * 256 + b` fold over the saltLength INTEGER) and :274-275 (`params.salt.unwrap_or(20) as usize` into `Pss::new_with_salt`).
- Scenario: the attacker signs with their own 2048-bit key and sets saltLength 0xFFFFFFFFFFFFFFDE. In rsa 0.9.10, `h_len + s_len + 2` wraps past the length check (pss.rs:187), and `split_at` (pss.rs:222) panics "mid > len". This runs for every signed field during the automatic validation at open (cms.rs:57 `verify_parsed`), and for chain checks of certificates. A 9-byte or longer INTEGER also overflows the fold in debug. In TS, WebCrypto rejects the call or returns false.
- Confidence: reproduced (release build, `crypto::verify_with_public_key`).
- Fix: use checked arithmetic in the fold. Treat a salt larger than modulus bytes minus hash length minus 2 as `Ok(false)` before calling `rsa`.

### H3. Dragging an annotation before its Created ack panics
- Where: `crates/plugins/annotation-core/src/update.rs:2018-2046` (`reconcile` renames the temp id in `by_id`, `order`, `selected` and `editing`, but not in `m.draft`). The panics are at update.rs:816 (`by_id[id]` in edit_up Move), :753 (edit_up Handle), :654 (edit_move Leader) and `snap.rs:48` (`annot_quad`, reached from edit_move Move when guides are on, which is the default).
- Scenario: draw a square (tmp:1, selected), then start dragging it before the engine's `Created{tempId: tmp:1}` arrives. Created renames tmp:1 in the model but not in the draft. `edit_draft_page` returns None, so the foreign-page guard does not stop the next sample either. The next pointermove indexes `by_id["tmp:1"]` and panics, which kills the wasm instance (the whole viewer) or aborts a native host. A second effect: `draft_ids` no longer locks the durable id, so an Upsert echo can overwrite it mid-drag. TS throws a TypeError at the same reads, so the bug is shared, but there it is recoverable.
- Confidence: verified (I re-read `reconcile` at the tip). How often it happens depends on Created latency, which is higher on a remote or slow engine.
- Fix: rewrite the draft's `id`/`ids` in `reconcile`, in TS and Rust. Turn the four index sites into `get` with a clear-draft fallback. Add a vector: create, pointer down, Created, move, up.

### H4. The form script transaction budget never runs out on a real host
- Where: `crates/plugins/form/src/scripting/controller.rs:127-133` (`remaining_budget`) and :140-145 (`started_at = rt::now()`, then `execution_ms += rt::now() - started_at`).
- What is wrong: `rt::now()` is the virtual clock (`crates/epdf-kernel/src/rt.rs:136-139`). Real hosts catch it up to wall time only when a drain starts (`crates/epdf-ffi/src/kernel_thread.rs:155-162`, `crates/epdf-web/src/runtime.rs` `catch_up`). A K/V/C/F chain is a run of synchronous `sandbox.run` calls separated by `rt::tick()`, so the whole chain runs inside one drain and every delta is 0. As a result, the "Form script transaction timed out" arm cannot be reached in production. TS uses `Date.now()`, which advances.
- Scenario: a hostile PDF puts N fields in /CO, each with a calculate script `while(true){}`. The host sandbox interrupts each run at the per-run 50 ms budget, but the transaction never stops early. Every value commit blocks the kernel thread for N x 50 ms (1000 fields is about 50 s per keystroke commit). TS caps the whole transaction at 50 ms. M9 makes this worse, because each faulted run also rebuilds and re-boots a realm.
- Why the tests miss it: the scripted conformance sandbox calls `advance` inside a run.
- Confidence: verified (I re-read both sides at the tip).
- Fix: measure with a real monotonic clock (`std::time::Instant`, or a host-injected `now` in the scripting options). Add a test where the sandbox burns wall time without moving the virtual clock. Check the actions executor for the same pattern.

## Medium

### Kernel and runtime

M1. Kernel promises settle 2 to 4 microtask turns earlier than TS. In `crates/epdf-kernel/src/kernel.rs`:
- :444-461 `begin_operation` uses one `rt::tick()`, where TS `.finally` costs 3 turns.
- :941-952 `open_document` returns the operation promise directly, where TS's async wrapper adds 2 adoption turns.
- :1122 `unlock_document_sync` uses `.map` (1 turn) where TS adds 2.
- :1517-1538 `retry`'s inner open inherits the open_document gap.

Scenario: with an engine that answers immediately, Rust `documents.open(A)` settles at run+1 and TS at run+5, so the `settled` order of open(A) against close(B) or another plugin chain in the same step can differ from the oracle. Confidence: verified by reading plus a Node turn measurement. Whether any current vector observes it is unverified; vectors record order only within a step. Fix: model `.finally` as 3 turns and wrap open/unlock in an adoption the way retry does. Add a vector that pairs open(A) with an independent chain.

M2. close() and destroy() join one turn early. `crates/epdf-kernel/src/kernel.rs:521-532` and :1452-1463 do `operation.await` / `start.await` (1 turn), where TS `await x?.catch(...)` costs 2. A close racing an in-flight open emits `closed` one turn early. Confidence: verified. Fix: add `rt::tick().await` in the `Some` arms.

M3. Awaiting an abortable promise is modelled as 1 turn, and "has an abort" is conflated with "is an AbortablePromise subclass".
- `crates/epdf-kernel/src/rt.rs:551-598` (`PromiseFuture`) charges 1 turn for every await. JS `await abortablePromise` costs 3. This affects the bring-up `render.policy` await (kernel.rs:835) and `DocumentHandle::request` (engine.rs:90-98).
- Conversely, `guarded.rs:88-91` and `rt.rs:461-471`, :520-531 set `abortable = true` on a guarded promise, which TS builds as a plain `new Promise` with an `abort` property. So `Promise::race` over `cx.doc()?.call(..)` adopts it with 2 extra turns.
- Confidence: verified by reading.
- Fix: split the flag into `has_abort` and `subclass`, and adopt (tick, await, tick) only for `subclass`.

M4. Closed sessions leak by default. `crates/epdf-kernel/src/kernel.rs:118-123`: `KernelOptions::default()` has `release_closed_sessions: false`, and `Rc<Session>` is held by every capability context that the session's own `capabilities`/`connectors` lists hold (plugin.rs:122, :474), so the references form a cycle. Both production hosts set it to true (`crates/epdf-viewer-host/src/session.rs:43`, `crates/epdf-web/src/exports.rs:361`). Any other embedder using the default leaks a Session per open/close cycle: handle, staged meta with the pages JSON, slice states and scope, surviving `destroy()`. The comment says "native callers rely on" the default, but no native caller does. Confidence: verified (I re-read the option at the tip). Fix: default to true, or hold the session weakly in contexts.

M5. A task awaiting a promise whose resolvers were all dropped is retained forever. `crates/epdf-kernel/src/rt.rs:50-53`, :100-106: tasks leave the thread-local map only when they reach Ready (or on `reset()`), and `Resolver` has no Drop. A plugin init promise abandoned without settling keeps the `start()` task, and through it `Rc<Inner>` (the whole kernel), alive after destroy. Confidence: verified by reading. Fix: reject with an "abandoned" error, or wake the waiters, when the last Resolver drops while pending.

### FFI, host and Swift

M6. A panic in a timer or a later task poll skips teardown and aborts. `crates/epdf-ffi/src/kernel_thread.rs:77` wraps only the synchronous `job` in catch_unwind. `drain(origin)` at :58, :72 and :84 (`rt::advance`, `rt::run_until_stalled`) is not wrapped, and most plugin work runs there. The unwind skips `bound::drop_all()` (:95), so the slab drops during TLS teardown, which bound.rs:22-25 documents as aborting. The test `a_panicking_job_tears_the_thread_down_cleanly` (tests/kernel_thread.rs:152) covers only the job path. Combined with H1 to H3, any plugin panic in async work kills the iOS app. Confidence: verified. Fix: catch_unwind around each drain, then the same teardown. Add a test that panics inside `rt::set_timeout`.

M7. `PDFViewer` deinit and destroy block the caller on two thread joins. Code path: `platforms/apple/EmbedPDF/Sources/EmbedPDF/PDFViewer.swift:72-75` and :27-31 call `host.shutdown()`, which joins the kernel thread (`crates/epdf-ffi/src/kernel_thread.rs:134-136`). The kernel thread's Session drop then joins the engine thread (`crates/epdf-viewer-host/src/session.rs:101`, `engine_worker.rs:106-116`), which waits out any in-flight PDFium call. Capabilities hold the viewer strongly, so deinit typically runs on the main actor when SwiftUI releases the model. Closing a document during a large render stalls the UI for the rest of that render. There is no deadlock, because nothing on the kernel thread waits on main. Confidence: verified (the stall length is plausible). Fix: shut down off the calling thread, or add a non-joining `shutdown_detached`.

### Wasm boundary (crates/epdf-web, packages/core-wasm)

M8. The engine bridge never removes closed documents. `packages/core-wasm/src/engine-bridge.ts:431` and :554: `documents.set` on open, and there is no `documents.delete` on close, so every opened and closed document handle lives until kernel destroy. `soak.test.ts` lists `documents` in its exact counts but never opens a second document, so it cannot catch this. Confidence: verified. Fix: delete the entry when the `close` call settles. Add a test that opens and closes and then checks `bridge.counts().documents`.

M9. An open aborted after the engine already resolved leaves the document open in the engine. Code path: `crates/epdf-web/src/engine.rs:139-147` and :182-201 decide the abort race on the Rust promise, which lags the JS promise by one microtask, and `engine-bridge.ts:552-556` turns the abort into a no-op once the engine promise has resolved. The late doc-id string carries no `$host` to release, so nothing closes it. Confidence: plausible (the window is narrow; the code path is verified). Fix: in `JsEngine::open`, close the late id when the result lands after abort. `tests/abort_release.rs` covers only `call`.

M10. Shim kernel status after destroy or a failed start diverges from the `Kernel` contract.
- `packages/core-wasm/src/kernel.ts:238-248` and `src/proxy.ts:260-263`, :511-518, :532-566.
- After free, every member throws. TS has `status()` return 'destroyed', `tryCapability` return null, `getState()` keep answering, and `start()` reject. Held snapshots' lazy `plugins.*` getters throw during a React unmount after destroy.
- The capability cache is keyed only on `core_revision`, so during `destroying` (kernel.rs:1448) and after a failed start (kernel.rs:1430-1432), cached proxies keep being served.
- Confidence: verified. kernel.test.ts:445-456 pins the throwing behavior as intentional.
- Fix: answer 'destroyed', null and the last state when dead; reject from `start`; fold status into the cache key.

M11. Rust rejections lose their error class. `packages/core-wasm/src/proxy.ts:181-189` (`toError`) builds a plain `Error`, so `isPluginError` and `isCancelled` from `@embedpdf/core` (`instanceof`) are false for every Rust rejection. A host that hides cancellations shows an error for close-while-opening. `cause`/`details` stay in canonical `{$error...}` / `{$undefined:true}` form instead of being decoded. Confidence: verified. Fix: rebuild `PluginError`/`CancelledError` from the spec and decode members through `Wire.decode`.

M12. Init-only JS plugins get a partial context and wrong timing. `packages/core-wasm/src/kernel.ts:184-226`:
- The ctx lacks `doc`, `documentHandle`, `getState`, `dispatch`, `subscribe` and `document`.
- Workspace init runs after Rust `start()` resolves and is not awaited, so a failing init never fails start.
- A rejected promise returned from `cleanup` becomes an unhandled rejection.
- The `cleanups` map keeps a key per closed document forever.

Confidence: verified. Fix: add the members or throw "unsupported", await init inside start, and catch cleanup promises.

### Plugins: scripting and actions

M13. A faulted realm is rebuilt and re-booted inside the same transaction, and a realm built after dispose is never disposed.
- `crates/plugins/acrojs/src/host.rs:190-197` (`note_fault` clears `sandbox` and `booted`) and :253-258 (`run_now` auto-boots) contradict the doc at :151-152 ("poison for the rest of the transaction"). Each interrupted run in a K/V/C/F chain therefore costs a new realm plus a full re-run of the hostile boot scripts.
- Separately, :318-327 checks `disposed` only at enqueue, and `ensure_sandbox` (:160-166) and `run_now` (:247-256) never check it. A transaction queued before document close builds a fresh sandbox after `dispose` (:329-341), and nothing ever disposes it: one QuickJS runtime of up to 16 MB per occurrence. Both form and actions reach this path, and so does the actions controller's own realm disposal at `crates/plugins/actions/src/controller.rs:150-155`.
- Shared with TS (host.ts:130-133, :151-157, :204-239).
- Confidence: verified by reading; two reviewers traced it independently.
- Fix: a per-transaction `poisoned` flag, and a `disposed` check in `ensure_sandbox`/`boot_now`/`run_now`. Change TS the same way so the vectors stay shared.

M14. A script's `print()` bypasses the `print` policy row. `crates/plugins/actions/src/scripting/executor.rs:259-267` and `surface.rs:58-99` send script Print effects to `fire_print_through_adapter` (`crates/plugins/actions/src/lifecycle/document_events.rs:84-135`), which checks the latch, authority and adapter, but never `decision_for`.
- Scenario: with defaults (print.lifecycle = block, javascript.lifecycle = allow), an OpenAction script calling `this.print()` opens the print dialog at load.
- TS shares the shape but is saved by `createDefaultActionsUiAdapter` (`packages/framework/web/src/actions-ui.ts:66-71`). Rust ships no default adapter, so a native host's naive `print` is exposed.
- Confidence: verified.
- Fix: gate on `policy.print.get(origin)` when a ui_context is present.

M15. PDF-supplied URIs reach `open_uri` unsanitized. `crates/plugins/actions/src/dispatch/run.rs:474-480` passes `str_of(node,"uri")` straight through, and the `ActionUiAdapter` docs (contract.rs:698-707) do not say the string is untrusted. TS filters with `sanitizeExternalUri` (actions-ui.ts:62), and Rust has no counterpart. A link to `javascript:` or `file:///etc/passwd` reaches a native adapter that may hand it to the OS opener. Confidence: verified. Fix: port the sanitizer as a pub helper and require it in the adapter docs.

### Plugins: annotation and annotation-core

M16. Hydration can get stuck until the document is reopened. `crates/plugins/annotation/src/sync/hydration.rs:69-76` and :142-151: on the no-permission or no-doc early return, `run_hydration` returns `Promise::resolved(())`, which `rehydrate` stores in `running`. Only a real run's `.finally` clears `running`. So after open-forbidden, then refresh, then access granted, a second refresh sees `running`, sets `again`, and returns the stale promise, and annotations never load. Shared with TS. Confidence: verified. Fix: do not store early-return promises in `running`, in TS and Rust, and add a vector for that sequence.

M17. Every annotation-core `update` deep-copies the whole model. `crates/plugins/annotation-core/src/update.rs` builds `Model { .., ..m.clone() }` everywhere (for example `with_draft` :614), and edit_up clones twice (:776, :797, :813). `by_id` is a plain `BTreeMap<Id, Annot>` that carries `serde_json` DTOs (types.rs:1266-1279), and `apply` clones again when nothing changed (:2051). That is O(N annotations plus their JSON) per pointermove at 60 to 120 Hz while dragging; TS shares structure. Confidence: verified. Fix: `Rc<Annot>` values or a persistent map. The cost is a public `Model` type change across dependents.

M18. annotation-core uses quadratic membership tests where TS uses Set. `update.rs:1924-1935` (`hydrate_annots` with `incoming.contains`/`reaped.contains` makes hydration O(n²); 20k annotations is about 4e8 comparisons) and :2010-2011 (`remove_annots`). Smaller instances: `draft_ids` at :1959, group.rs:81 and snap.rs:79. Confidence: verified. Fix: HashSet.

M19. The comment thread index is rebuilt from scratch on every read, at quadratic cost. `crates/plugins/annotation/src/comments/threads.rs:38-97` and `comments/build.rs:241-334`: there is no memo (TS memoizes on byId/order identity), and lookups are linear Vecs. `reply` rebuilds twice. A sidebar calling `get_thread` per thread is O(N³) per render. Confidence: verified. Fix: HashMap plus a memo keyed on `Rc::ptr_eq` of the model.

M20. Untrusted geometry drives unbounded loops and allocations. `crates/plugins/annotation-core/src/cloudy.rs:375-391` (`flatten_ellipse` pushes `max(32, ceil(max(rx,ry)*2))` points), :363-369 (curls per edge) and `scene.rs:51-55` (`squiggle_path`, `w/half` iterations). A 1e12-point rect requests a Vec of 1e12 Points, and a failed allocation aborts with no possible catch. Past 2^53 the float loops stop advancing. Inputs come from PDF /Rect, /QuadPoints and /BE, and from unvalidated `CreateAnnot` geom (update.rs:2059). Shared with TS. Confidence: verified for the loops; whether the engine clamps rects first is unverified. Fix: cap the counts and skip non-finite input.

### Plugins: form, selection, signature, render, stage

M21. Every form model update deep-clones the whole form snapshot. `crates/plugins/form/src/core/model.rs:44` (`snapshot: Option<Value>`), with clones at :121, :132, :143, :162, :167, :178. Also `read/fields.rs:25-27` and :65-67, where `list_fields`, `export_values` and `get_snapshot` each clone the full array. One `set_value` costs three full copies; TS shares by reference. Confidence: verified, not measured. Fix: `Rc<Value>` for the snapshot and index maps, with borrowing read helpers.

M22. Selection page-range work is O(N²), and a select-all settles at O(N³). `crates/plugins/selection/src/controller.rs:261-266` (`pages()` deserializes the whole registry), :277-280 (`pon_at_index` calls it per index), used in loops at :421-422 (`recompute`) and :819-820 (`read_text_in_range`). `select_all` on 1,000 pages costs about 1e9 row decodes before it settles; TS is O(1) per lookup. Confidence: verified. Fix: decode once per call, or keep a pon-to-index map keyed on `Rc::ptr_eq`, as measurement does (`crates/plugins/measurement/src/services/store.rs:111-131`).

M23. A text read in flight can return pre-redaction text. `crates/plugins/selection/src/controller.rs:565-591`, :796-838 and :614-621: geometry loads check `epoch` (:340) but text reads do not, so a 20-page `read_text` racing `redaction.applied` returns redacted text for the batches already in flight. Shared with TS (controller.ts:447-493). Confidence: verified by reading. Fix: capture the epoch at entry and reject if it moves across any await.

M24. The signature plugin has no public event hooks. `crates/plugins/signature/src/services/events.rs:17` (`SignatureEvents` is `pub(crate)`), and no `on_*` method exists in the crate. TS exposes nine hooks (contract.ts:250-258), from `onSigned` through `onInspectionRequested`. A native host cannot receive `sign_requested` from an armed mark (armed_mark.rs:83-88), so its sign dialog never opens. Confidence: verified. Fix: add hooks the way `crates/plugins/stamp/src/services/events.rs:71-116` does.

M25. A tile fetch that fails synchronously strands its entry and its in-flight slot. `crates/plugins/render/src/tile_manager.rs:660-685`: `in_flight += 1` (:660) and the charged entry insert (:672) happen before `store.acquire(...)?` (:674). An engine that refuses a page still in the registry (a page-delete race) leaves the entry unresolved forever. After 8 such failures, `MAX_IN_FLIGHT` stops tile fetching for every page of the document. Shared with TS (tile-manager.ts:494-508). Confidence: verified. Fix: charge only after acquire succeeds, or release and mark failed on Err.

M26. An empty render ladder or empty tile pyramid panics. `crates/plugins/render/src/strategy.rs:111` (`widths[widths.len()-1]`), `tiles.rs:146` and `tile_manager.rs:274` (`pyramid[0]`): the `unwrap_or` argument is evaluated eagerly, so an empty list panics before `find` runs. Neither `Quantize::Ladder(vec![])` (also reachable over FFI as `RenderQuantize::Ladder{widths: vec![]}`) nor a cloud policy with `tiles.scales: []` is rejected. TS degrades to undefined or NaN. Confidence: verified. Fix: validate in `resolve_render_options`/`resolve_strategy`.

M27. Non-finite camera input in a grid layout panics. `crates/plugins/stage/src/camera/write.rs:173-174`, `services/scene.rs:433-435` and :223, `camera/gestures.rs:75-76`, `navigation/zoom.rs:59-60` all `.expect("a scene with items has an anchor")`, but `crates/plugins/stage-core/src/lib.rs:654-664` also returns None for a NaN or fractional grid index. `set_camera({x: NaN})`, `zoom_by(NaN)` or `columns: Count(2.5)` all panic, where TS throws a catchable error. Confidence: verified. Fix: reject non-finite or fractional values at the public verbs, or return Result.

### Conformance harness

M28. Numeric tolerance hides millisecond errors in epoch values. `crates/plugin-conformance/src/canonical.rs:11-19` scales 1e-12 by max(|a|,|b|,1), so 2559009906002 equals 2559009906000. Signature `signingTime`, `$date`, `genTime` and actions/stamp `nowMs` are all in this range. An off-by-one-ms date conversion passes. Confidence: verified. Fix: compare exactly when both values are integers below 2^53.

M29. `snapshot: 'none'` turns off state comparison for 572 of 1,273 scenario cases. Honoured at `tooling/plugin-conformance/src/scenario.ts:244` and `crates/plugin-conformance/src/scenario.rs:481-489`. Examples: form-scripting 43/46, form-design 23/29 (`init-with-annotation` dispatches 34 actions and compares no state), render-capability 32/38, page-edit 17/19. coverage.ts still counts those reducer lines as covered. All 103 dispatched action types appear in at least one snapshotting case, which limits the damage. Confidence: verified. Fix: require a reason for 'none', or have coverage ignore lines reached only by snapshot-none cases.

M30. Orphaned or unreplayed vector files go unnoticed. `tooling/plugin-conformance/src/vectors.ts:100-125`: `vectors:check` compares only the files that current suites write. A deleted TS suite's JSON stays green, and a new suite with no Rust `assert_suite` stays green. All 109 files are referenced today (checked by hand). Fix: fail `--check` on unwritten files, and add a Rust test that every vector path is replayed.

### Build, CI and release

M31. The wasm kernel is never built or tested in CI. `.github/workflows/*.yml` never mention `core-wasm`, `epdf-web` or `wasm32-unknown-unknown`. `crates/epdf-web/tests/*.rs` are all `#[wasm_bindgen_test]`, so engine-rust.yml:85 `cargo test --workspace` reports "0 passed" for each of the 8 binaries and goes green. `packages/core-wasm/package.json:11-14` has no `test` script, so turbo skips its 59 vitest and 10 GC tests. Confidence: verified (the reviewer ran `cargo test -p epdf-web` on the host). Fix: a path-filtered job that installs wasm-bindgen-cli 0.2.127 and binaryen, runs `crates/epdf-web/scripts/build.sh`, `cargo test -p epdf-web --target wasm32-unknown-unknown` with the test runner, and `pnpm --filter @embedpdf/core-wasm test:wasm`.

M32. `wasm-opt` is an undocumented build prerequisite. `crates/epdf-web/scripts/build.sh:13-16` calls binaryen's `wasm-opt`, which neither `examples/rust-kernel-eval/README.md:9-19` nor `perf-compare/scripts/doctor.mjs:25` mentions. On a clean machine, the build fails after the full cargo build and after wasm-bindgen has already written `wasm/`. Confidence: verified. Fix: document binaryen 133 and add a doctor check (or `command -v` in build.sh).

M33. `scripts/mutants.sh:42` traps signals without exiting. `trap '...' EXIT INT TERM HUP` resumes the script after the handler. A waiter that gets SIGTERM removes its ticket and then loops forever (:62). A run hit by SIGTERM during `cargo mutants` (:74) deletes `$LOCK` while cargo keeps running, which allows the concurrent run the script exists to prevent. Confidence: reproduced with a minimal sh script. Fix: `trap 'cleanup; exit 130' INT TERM HUP`, plus `exec` cargo or forward the signal.

M34. No changeset for a published package. `packages/framework/react/package.json` is `"private": false`, and `src/toolbar.tsx:501-566` and `src/stage.tsx:383-391` change runtime behavior, but the branch adds no `.changeset/*`. Confidence: verified. Fix: one `'@embedpdf/react': patch` file.

## Low

Kernel (`crates/epdf-kernel`):
- L-K1 `src/action.rs:21-27`: `action_type()` serializes the whole action per call, and `plugin.rs:445` calls it once per listener per dispatch, so the cost is O(listeners x payload). `js.rs:27` panics on unserializable keys. Compute the type once in `emit_action`.
- L-K2 `src/kernel.rs:1010-1014`: a synchronous throw from an open thunk goes through `engine_call`'s race and lands about 3 turns later than in TS. Return `Err` directly.
- L-K3 `src/kernel.rs:836` and :1050-1051: `{"$undefined":true}` counts as present (`!is_null()`), so `password: undefined` marks `passwordProvided: true`. Also check `is_undefined()`.
- L-K4 `src/signal.rs:50-62`: `abort` still calls a listener removed by an earlier listener during the same dispatch, unlike the DOM. Tombstone, as `store::Listeners` does.
- L-K5 `src/lanes.rs:105-114`: `LatestLane::cancel` clears `active` before aborting, the reverse of TS, so `running()` inside an abort listener differs.
- L-K6 `src/plugin.rs:1015-1026`, :846-849: `.reduce()` without `.initial_state()` silently drops every action and then panics with a misleading "slice was removed". Require `initial_state`.
- L-K7 `src/rt.rs:182-184`, :219-229: `set_interval(0.0)` makes `advance` loop forever. Clamp to at least 1 ms.
- L-K8 `src/kernel.rs:974`, :543: a ready document keeps its full open input (inline `$bytes`) until close. Drop it in `commit_ready` unless the host hold needs it.
- L-K9 `README.md` mutation section cites stale lines (for example "kernel.rs:419/420", now about :470-472). Separately, the SAFETY comment at `rt.rs:93-97` claims a cross-thread wake is a no-op, but per-thread task ids can collide and wake an unrelated task.

Wasm boundary:
- L-W1 `crates/epdf-web/src/exports.rs:365`, :475: `kernel.documents !== kernel.capability(DocumentsToken)`, giving two proxies and two pinned ref sets. Resolve through `capability_any(DOCUMENTS)`.
- L-W2 `packages/core-wasm/src/proxy.ts:356-370`, :394-432: JS functions encoded where Rust never makes a callback stay in `fns` until destroy (ignored config members, error members, in-flight ids). Plausible unbounded growth with fresh inline closures.
- L-W3 `proxy.ts:553-565`: an old snapshot reads current slices it had not read yet, so `prev.plugins.x === next.plugins.x` across a real change. Documented, but TS snapshots are immutable.
- L-W4 `crates/epdf-web/src/exports.rs:430`, :448, :460 calling `crates/plugin-conformance/src/harness.rs:271-278`: unknown token names grow the token table permanently.
- L-W5 `packages/core-wasm/src/engine-bridge.ts:130-143`, :195: `errorSpec` recursion runs before the `seen` cycle guard, so a self-referencing Error causes a RangeError that replaces the engine error.
- L-W6 `proxy.ts:54`, :479-483, :200-201: misleading comments (cache clearing on callJs, "recursive use" hazard, an orphaned doc comment).
- L-W7 Tests: nothing covers `tryCapability` during destroying, or error class across the boundary.
- L-W8 Architecture note: `crates/epdf-web/Cargo.toml:13` depends on `plugin-conformance`, and exports drive the kernel through `Harness::live`, the conformance fake's `engine_error`, and every plugin's default `harness` feature. The crate says "Evaluation only", which is fine for this branch, but it means the shipping-shaped wasm carries the test harness and its JSON dispatch. Any move toward production needs a separate adapter layer. Confidence: verified (I read Cargo.toml and exports.rs at the tip).

FFI, host and Swift:
- L-F1 `platforms/apple/EmbedPDF/Sources/EmbedPDF/EventRelay.swift:40-49` and `EmbedPDFManualBridge/ManualSearch.swift:60-65`: `onTermination` is set after subscribing, so a stream on an already-closed document finishes first, `remove(id)` and `epdf_subscription_free` never run, and the registry entry or Box leaks. Plausible (it relies on AsyncStream semantics). Set the handler first.
- L-F2 `EmbedPDFManualBridge/ManualCall.swift:79-94`: a cancel that lands before `callID` is stored is lost. Keep a cancelled flag in the same lock.
- L-F3 `ManualSearch.swift:149-151`: `raw.hits[position]` has no bounds check on a public collection. Add a precondition.
- L-F4 `crates/epdf-ffi/src/kernel_thread.rs:148-150`: a failed `send` drops the job while the post mutex is still held. Latent re-lock if a job's Drop posts. Clone the Sender out of the lock first.
- L-F5 `crates/epdf-manual-bridge/src/lib.rs:176-211`: the ABORTS table is not drained before thread exit, so Completions can fire during TLS destruction (the hazard `drop_all` exists to avoid).
- L-F6 `EventRelay.swift:29`: `StateRelay.report(error:)` is empty, so every kernel error report is dropped. Add an `errors` stream or os.Logger.
- L-F7 `crates/epdf-viewer-host/src/lib.rs:210`: `documentEvents` never ends when `events()` is None.
- L-F8 `platforms/apple/EmbedPDF/Sources/EmbedPDF/Exports.swift:4`: the public `PDFDocument` typealias collides with PDFKit. `PDFViewer.unlock` (PDFViewer.swift:45) is public but cannot succeed.
- L-F9 `platforms/apple/EmbedPDFDemo/EmbedPDFDemo/DocumentModel.swift:101-112`: a partially opened document leaks if a later capability throws, and so does each wrong-password attempt.
- L-F10 `crates/epdf-manual-bridge/src/render.rs:109`, `search.rs:243`, :263, :291, :332, :447: `&*ptr` without a null check (documented precondition; a null aborts).

Actions, form, acrojs:
- L-A1 `crates/plugins/actions/src/host_contract.rs:195-216`: `DetachedScriptRealm` has no Drop and is not tied to the owner document, so a realm dropped without `dispose()` leaks its sandbox.
- L-A2 `crates/plugins/actions/src/lifecycle/open_sequence.rs:111-115`: the open-sequence op does not reset `script_nodes`, so 16 hover JS nodes before the adapter installs make the OpenAction JS inert. Shared with TS (open-sequence.ts:118).
- L-A3 `crates/plugins/actions/src/dispatch/triggers.rs:79-102`: the lifecycle-tree cache can store a list read before an invalidation. Use a generation counter. Shared with TS.
- L-A4 `crates/plugins/actions/src/dispatch/dispatcher.rs:236-238`: `can_dispatch` returns false where TS throws not-ready. Unreachable in practice; record it in the README.
- L-A5 `crates/plugins/form/src/write/values.rs:202-206` and `read/fields.rs:114-122`: `import_values`/`export_values` use insertion key order, while JS puts integer-like keys first, so K/V/C/F order and event order differ for names like "10" and "2".
- L-A6 `crates/plugins/form/src/write/fields.rs:42-58`: `auto_name` is quadratic (`Vec::contains` with a `format!` per probe).
- L-A7 `crates/plugins/acrojs/prelude.js:436-447`: `util.printf("%.500f")` throws a RangeError inside the sandbox. Shared with TS; clamp the precision in prelude.ts.

Annotation:
- L-N1 `crates/plugins/annotation/src/services/store.rs:58-62`: `draftChanged` fires on value change in Rust and on identity change in TS. Plausible; no vector covers it.
- L-N2 `write/text_editing.rs:67-81` and `tools/handlers.rs:518-535`: debounce and ink-group timers are not cleared at close, so a free-text edit typed less than 250 ms before close is lost, and the ink timer dispatches into a closed instance. Shared with TS.
- L-N3 `read/render.rs:158-166`: the abort listener in `render_appearances` is never removed and retains image bytes per call on a long-lived signal.
- L-N4 `write/crud.rs:597-602`: rollback of patch A can erase a newer confirmed patch B. Shared with TS.
- L-N5 `comments/threads.rs:63-74`: the sort comparator is not total with a NaN `top`, which can panic on Rust 1.81 and later. Use `total_cmp`. annotation-core has the same pattern at `measurement_shape.rs:210` and `hit.rs:221-223`.
- L-N6 `repository/mod.rs:65`: a DTO with no parsable ref gets id "", so such records collapse into one entry. Reachable from remote events (`sync/remote.rs:36-46`).
- L-N7 `write/selection.rs:143-155`: `rotate_selection_by(45.0)` rotates by -90.
- L-N8 `comments/comments.rs:224-227`: `delete_thread` silently skips members whose ref does not deserialize, leaving orphans.
- L-N9 `write/script_effects.rs:123`, :162: script commits send the model subtype (`widget-text`) instead of the wire subtype. Shared with TS.
- L-N10 `write/drafts.rs:16-21`: `is_create_draft` serializes the whole draft per query.
- L-N11 `read/selection_props.rs:68-71` with `rich_text.rs:457-470`: `fontSize: null` and `fontColor: ""` where TS omits the key or throws.
- L-N12 Coverage: annotation-core has no vector for messages that arrive mid-gesture (the H3 gap), and no bounded-output case for the path generators.

Selection, search, link and others:
- L-S1 `crates/plugins/selection/src/controller.rs:265`: `unwrap_or_default()` means one malformed registry row empties the whole page list.
- L-S2 `crates/plugins/selection/src/text_layout.rs:334`, :358, :571: a non-tiling geometry snapshot from a non-PDFium host indexes out of bounds and panics. Validate in `build_page_text_layout`.
- L-S3 `crates/plugins/selection/src/controller.rs:645`, :672: `i64` overflow on `start + count` and `end_index - 1`. Use saturating arithmetic.
- L-S4 `controller.rs:597-621`, `model.rs:85-93`: `loaded` is never cleared on invalidate, and `text_snapshots` for pages read but never hovered are never evicted. Shared with TS.
- L-S5 `crates/plugins/link/src/controller.rs:314`: `get_label` takes only a target, so a `Link` input answers "Link". The harness unwraps it (harness.rs:137), so conformance passes while the public API differs.
- L-S6 `crates/plugins/link/src/controller.rs:139-160`: a rejected load is never removed, so the page can never retry. Shared with TS and documented.
- L-S7 `crates/plugins/search/src/ffi.rs:463`: the doc says "document order" but the order is ascending page object number.
- L-S8 `crates/plugins/selection/src/contract.rs:51-55`: a `SearchHit` (f64 fields) cannot be passed to `select` (i64) without hand casting.
- L-S9 `crates/plugins/selection/src/controller.rs:128-148`: a private `Promise.all` that hangs on empty input. Reuse the shared helper.

Render and stage:
- L-R1 `crates/plugins/render/src/controller.rs:603-609`: the abort listener retains the task, so each cached raster holds an uncosted second copy of its image JSON. This doubles memory for inline `$bytes` engines.
- L-R2 `crates/plugins/render/src/raster_store.rs:103`, :233-237: a bytes handle without `$byteLength` costs NaN, which disables the LRU permanently (`total` becomes NaN). Current producers always add the field.
- L-R3 `crates/plugins/stage/src/controller.rs:107`, :162-173: `providers` and `rules` closures are not cleared at close, so a closure capturing the stage capability leaks the stage `Inner` per open/close. Interaction already handles this (`interaction/src/controller.rs:168-177`, `Hub::release` at `:182`).
- L-R4 `crates/plugins/stage/src/lib.rs:125-148`: the goto executor returns a `Failed` result where TS throws for a destination-less node.
- L-R5 `crates/plugins/stage/src/services/scene.rs:49-58`: `filter_map(.ok())` drops unparseable registry rows, which shifts indices, and then :223 panics.
- L-R6 `crates/plugins/stage/src/motion.rs:41`, :64, :69: `powf` may differ from V8 `Math.pow` by one ulp. Plausible.
- L-R7 `crates/plugins/render/src/tiles.rs:59-75`: `tiles.size = 0` loops forever until out of memory. Shared with TS.

Signature, stamp, redaction:
- L-G1 `crates/plugins/signature-core/src/crypto.rs:122`: `RsaPublicKey::new` rejects keys over 4096 bits, and the error maps to `invalid` rather than `unsupported`. A legitimately signed 8192-bit document reads as tampered.
- L-G2 `crates/plugins/stamp/src/convention.rs:45`, `libraries.rs:390`, :185-188, :342: asset ids `lib:name` can collide when names contain `:`, or when duplicate PieceInfo names are imported. Shared with TS.
- L-G3 `crates/plugins/redaction/src/write/marks.rs:69-73`: malformed `bounds` reject with a bare serde error, not a PluginError.
- L-G4 Coverage: no adversarial CMS tests (deep nesting, PSS salt, large keys), and no build without the harness that would have caught M24.

Commands, i18n, shell, ui-core:
- L-U1 `crates/plugins/commands/src/contract.rs:254` vs `crates/plugins/ui-core/src/projection.rs:37`: two `MenuTarget` types, so `get_menu_target` cannot be passed to `project_overflow`. Re-export the ui-core type.
- L-U2 i18n, shell, ui-core and commands do not declare serde_json `preserve_order`, so object key order depends on which other crates are linked (`crates/plugin-conformance/Cargo.toml:18` enables it under the harness). The comment at `crates/plugins/i18n/src/harness.rs:10-13` is wrong.
- L-U3 `crates/plugins/i18n/src/loaders.rs:45`, :88-98: a loader that throws synchronously leaves `setLocale` unsettled, and a retry with the same code is a no-op (model.rs:119-124). Shared with TS.
- L-U4 `crates/plugins/commands/src/controller.rs:54-59`, :380-386: a `CommandDef` closure that captures the capability forms an Rc cycle that is never broken. Plausible.
- L-U5 `crates/plugins/commands/src/controller.rs:155-165`: partial `register_commands` failure leaves earlier registrations with no remover. Shared with TS.
- L-U6 Three JS-record implementations (`i18n/src/js.rs:75`, `shell/src/record.rs:50`, `ui-core/src/js.rs:19`), and `OBJECT_PROTOTYPE_MEMBERS` is duplicated.
- L-U7 Removers return `Rc<dyn Fn()>` (`commands/src/controller.rs:140`, `i18n/src/controller.rs:181`) where `on_*` returns `Unsubscribe`.
- L-U8 `crates/plugins/ui-core/src/harness.rs:110-159`: `args[i]` indexing panics on a malformed vector.

Conformance:
- L-C1 `tooling/plugin-conformance/src/canonical.ts:63-79`: non-Error throws are canonicalized like `Error`, and member order is not compared (canonical.rs:62-79).
- L-C2 i18n plurals and render-tiles `localeCompare` depend on Node's ICU version, while CI pins only `node-version: 24` (plugin-rust.yml:100). Pin the version or check `process.versions.icu`.
- L-C3 `crates/plugin-conformance/src/probe.rs` (abortSignal) drops the abort reason, so reason propagation is untested.
- L-C4 Scripted callbacks and fake-engine answers decode `$` tags in TS (scenario.ts:480-481, fake-engine.ts:204) but stay raw in Rust (harness.rs:626-629, fake_engine.rs:216).
- L-C5 `scenario.ts:111-114`: `registerPlugin` silently overwrites a duplicate name.
- L-C6 Module-global shims (stamp-fixtures.ts, annotation-fixtures.ts:44-55, actions.ts:44-53) keep pointing at a finished case's env, so later console output is lost.
- L-C7 `suites/stamp-fixtures.ts:219-236` re-declares `stamp.plugin.ts` inline and can drift.
- L-C8 `coverage.ts:52` uses `--all=false`, so an unloaded ported file never fails the gate.
- L-C9 Dead code: `scenario.rs:158-159`, :301-305, and `Run.unmatched` (never read). `harness.rs:37` silently turns a serialization failure into null. The README's awaitCalls claim is wrong.

Build, docs, perf:
- L-B1 `multiplatform-plugins-status.md:17-18` says P6 (viewer-host, uniffi) has not started, but it ships in this branch. It also says 20 plugin packages; there are 19.
- L-B2 `perf-compare/test/config.test.mjs:27` expects 41 arms, but `arms.json` has 43, so the test fails (reproduced). README says "Six arms". Keep perf-compare only on `research/viewer-perf-compare`.
- L-B3 Root clutter (`multiplatform-plugins-*.md`, the 1,704-line `viewer-perf-compare-plan.md`). Personal hostnames are committed: `docs/research/swift-bridge-benchmarks.md:244` (a tailnet hostname), `perf-compare/README.md` and `docs/superpowers/plans/2026-10-01-swift-package.md`. Another developer's path `file:///Users/bobsingor/...` is in the plan. The rust-kernel docs live in `examples/viewer-react`.
- L-B4 `.github/workflows/plugin-rust.yml:27-39`: push paths omit `rust-toolchain.toml`, which the PR paths include.
- L-B5 `perf-compare/fixtures/large.pdf` (1 MB) can be regenerated by `generate.mjs`. Committing it is a documented choice.
- L-B6 `crates/plugins/.gitkeep` is redundant.

## Coverage ledger

| Area | Files | Depth | Reviewer |
|---|---|---|---|
| crates/epdf-kernel | 26 | Read fully: all src (kernel.rs, plugin.rs, rt.rs, store.rs and the rest), README, tests/next_deadline.rs, tests/state_text.rs. Skimmed: tests/runtime.rs | kernel reviewer |
| crates/epdf-web | 14 | Read fully: src/*, Cargo.toml, scripts/build.sh, tests abort_release, host_release, clock, runtime. Skimmed: tests exports, engine, host_bytes | wasm reviewer |
| packages/core-wasm | 40 | Read fully: all src/*.ts, configs, soak.test.ts. Sampled: redirect/ (2 of 22, the rest identical re-exports), kernel.test.ts, release.test.ts. Test names only: the other tests | wasm reviewer |
| packages/engine | 10 | Diff read fully except `pdf-functions.generated.ts` and the runtime-src submodule bump (not reviewed) | wasm reviewer |
| crates/plugins/annotation | 71 | Read fully: controller, model, services, sync, the main write/read/comments/tools files, repository/mod and projection. Sampled: build.rs, stamps, measurement, markup, handlers, rich_text, seam, quads, harness. Skimmed: contract, host_contract, chrome, ghost, icons, pointer, placement, settings, links, props, kinds/*, text_item, definitions, README | annotation reviewer |
| crates/plugins/annotation-core | 30 | Read fully: about 22 src files, compared with TS. Partial: measurement_shape.rs, types.rs, js.rs. Not read: kinds.rs (covered by 137 vectors). Skimmed: harness | annotation-core reviewer |
| crates/plugins/form | 39 | Read fully: all non-test src and tests/api.rs. Skimmed: harness. Not read: tests/conformance.rs | form/acrojs reviewer |
| crates/plugins/acrojs | 12 | Read fully: src/*. Sampled: prelude.js, harness. Not read: tests/conformance.rs | form/acrojs reviewer |
| crates/plugins/actions | 33 | Read fully: all src, harness, tests, README | actions reviewer |
| crates/plugins/stage | 31 | Read fully: all src except harness (first 140 of 763 lines), tests | stage/render reviewer |
| crates/plugins/stage-core | 5 | Read fully: lib.rs. Skimmed: harness, tests | stage/render reviewer |
| crates/plugins/render | 21 | Read fully: core src. Sampled: paint_plan.rs, js.rs. Not read: harness, tests/ffi.rs, tests/support | stage/render reviewer |
| crates/plugins/view-manager | 10 | Read fully: lib, contract, model, controller. Not read: record.rs, harness, tests | stage/render reviewer |
| crates/plugins/interaction | 11 | Read fully: lib, model, callbacks, feedback, controller. Not read: harness, tests | stage/render reviewer |
| crates/plugins/signature | 27 | Read fully: all src except harness (grep only) | signature reviewer |
| crates/plugins/signature-core | 18 | Read fully: der, parse, cert, crypto, cms, gate, sign, verdict, trust. Sampled: policy.rs, signers.rs. Grep only: oids.rs. Not read: tests | signature reviewer |
| crates/plugins/stamp | 26 | Read fully: all src except harness (grep only). Not read: tests | signature reviewer |
| crates/plugins/redaction | 18 | Read fully: all src except harness (grep only) | signature reviewer |
| crates/plugins/selection | 13 | Read fully: all src. Sampled: harness | selection reviewer |
| crates/plugins/search | 12 | Read fully: controller, model, lib, contract, ffi. Not read: token.rs, harness, tests/support | selection reviewer |
| crates/plugins/measurement | 24 | Read fully: controller, store, events, viewports, write/scale. Grep only: units, scale, text, create. Not read: calibration, read/scale, contract, model, harness | selection reviewer (gap) |
| crates/plugins/geometry | 6 | Sampled: lib.rs, js.rs. Not read: harness, tests | selection reviewer (gap) |
| crates/plugins/link | 10 | Read fully: controller, source, model, lib. Not read: contract, token | selection reviewer |
| crates/plugins/page-edit | 7 | Sampled: controller. Not read: contract, harness, lib | selection reviewer (gap) |
| crates/plugins/metadata | 9 | Read fully: controller. Grep only: ffi.rs. Not read: model, lib, harness, tests | selection reviewer (gap) |
| crates/plugins/commands, i18n, shell, ui-core | 44 | Read fully: everything except plural_data.rs (generated, sampled) | commands reviewer |
| crates/plugin-conformance | 10 | Read fully | conformance reviewer |
| tooling/plugin-conformance | 67 | Read fully: runner, comparator, generator, coverage, fake engine, probe. Sampled: 7 suites. Grep only: the other 50 suites. Vector JSON: format sampled, counts scripted | conformance reviewer |
| crates/epdf-ffi | 12 | Read fully: src. Test lists only: tests (the panic test read fully) | FFI reviewer |
| crates/epdf-viewer-host | 14 | Read fully: src. Sampled: tests/viewer_host.rs. Test lists only: other tests | FFI reviewer |
| crates/epdf-manual-bridge | 8 | Read fully: src and header. Test list only: tests/layout.rs | FFI reviewer |
| platforms/apple | 45 | Read fully: all EmbedPDF, EmbedPDFUI and ManualBridge Swift sources, build script, Package.swift, main demo models, core tests. Not read: some NutrientDemo and EmbedPDFDemo views, SmokeTests | FFI reviewer |
| examples/rust-kernel-eval | 7 | Read fully | FFI reviewer |
| crates/plugins/*/src/ffi.rs (render, search, metadata) | 3 | Search and metadata grep only; render ffi.rs read. KernelBound use across FFI was not cross-checked by the FFI reviewer | gap |
| .github/workflows, Cargo.toml, toolchain, mutants, pnpm | 9 | Read fully. Cargo.lock checked with `cargo metadata --locked` | build reviewer |
| packages/framework, packages/plugin, examples/viewer-react | 6 | Diffs read fully. viewer-react docs sampled | build reviewer |
| docs/superpowers, docs/research, root plan and status docs | 9 | Status doc read fully. Others: path sweep and grep only | build reviewer (skimmed) |
| perf-compare | 30 | Harness core read fully. Report, build-arm, env, README sampled. FINDINGS.md, RESULTS.md, arms.json, entry, gate not reviewed, per scope | build reviewer (light) |
| perf-compare/fixtures, JSON vectors | n/a | Not reviewed as content (generated) | none |

The largest gaps are measurement, geometry, page-edit and metadata (controller level only), the plugin `ffi.rs` handles, the harness.rs files of most plugins (test adapters), and the docs. Before merge, the harness files deserve a second look for one reason: epdf-web ships through `Harness::live` (L-W8), so harness adapter bugs are user-facing on the web build.

## Strengths worth keeping

- RefCell discipline is consistent across the kernel and every plugin reviewed: contents are cloned or taken out before any callback, emit or await runs. No reviewer found a reachable double borrow. `store::Listeners` reproduces JS Set iteration under mutation with tombstones, and it is tested.
- Lifetime handling is mostly deliberate. Removers, timers and disposers hold `Weak`. Interaction breaks the handler-to-hub cycle at close. Render frees host bytes on the last handle drop. In epdf-ffi, `KernelBound`/`PromiseSlot` keeps the `!Send` kernel on its thread, with a lock-free cancel CAS and tested teardown ordering (`drop(rx)` before `drop(state)`, and self-shutdown that detaches).
- The conformance comparator is strict and symmetric: missing and extra keys both fail, and undefined, NaN and -0 have explicit encodings. It logs engine calls, dispatches and settlements, not just final results. There is no skip list or `#[ignore]`, CI gates on `vectors:check` and `coverage:check`, and every reducer action type appears in at least one state-comparing case.
- JS numeric and string semantics are ported precisely (`to_fixed`, `number_to_string`, UTF-16 slicing, NaN-propagating min/max, Intl.PluralRules from CLDR 48). The READMEs list every intended divergence and the mutation-testing survivors, which makes parity claims auditable.
- The FFI C header pins its layout with `_Static_assert`s checked against the Rust `repr(C)` twins, and `translate.rs` validates stride times height against the payload length before any pointer reaches Swift.
- Redaction is a thin port. It owns no mark state, applies only through the engine's `redaction.apply`, and checks the full capability set in `can_apply`.
