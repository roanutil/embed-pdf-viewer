# CU-C4 decisions

Decisions made while adding engine vectors for upstream's new engine behaviour in
`074a190c9..ed064e0c8` and porting `epdf-engine` and `pdfium` to replay them, on
`research/catchup-2026-11-12-c4`. Each names the commit that carries it; "the docs commit" is the one
that adds this file. None of them is a NEW DIVERGENCE, and nothing under `packages/**` changed.

## CU-C4-D1. One new vector group, recorded at the worker boundary

Decision: the new cases live in their own group, `changes` (`vectors/changes/*.json`, generator
`src/generate-change-vectors.ts`, scripts `changes` and `changes:check`), rather than in `pages`,
`annotation` or `forms`. Each case records every worker request a public-API call sent and the
worker's answer to it (`dec72ba45`, `2fba1ac5b`).
Reason: the behaviour arrives through `doc.apply` and `pages.list`, and the worker is what the Rust
`Host` replaces, so the worker's answer is the level the two engines can be compared at without the
main thread's reshaping. A separate group regenerates without touching the 33 existing ones.
Cost: one more generator and Rust runner (`tests/change_ops.rs`) to keep up; `regen-engine.sh` and
`suite.sh` pick it up through its `:check` script without edits.
Reverse: delete the generator, the two scripts, `vectors/changes/` and `tests/change_ops.rs`.

## CU-C4-D2. A new fixture with `/VP`

Decision: `fixtures/generate.mjs` gains `viewportPages()`, written to `fixtures/viewports.pdf`: a page
with a foreign and an owned rectilinear viewport, a page with none and a crop box, and a page turned a
quarter with a geospatial viewport (`dec72ba45`).
Reason: no fixture had `/VP`, and these three pages reach every branch of `readViewports`,
`viewportsWithScale` and `scaleChanges.ts` the cases name. Every other fixture regenerates
byte-identical.
Cost: one 1,417-byte fixture.
Reverse: drop the function and its output line, and the file.

## CU-C4-D3. Mask made-up names and clock timestamps by rule

Decision: the generator records a UUIDv7 as `"<uuid>"` and an ISO timestamp later than 2025-01-01 as
`"<now>"`; `tests/change_ops.rs` masks its own answers the same way.
Reason: a copy gets a fresh UUIDv7 name and every write stamps `modifiedAt`. Every date the fixtures
carry is from 2024, so the rule separates the clock's values from the file's without a list of keys.
Cost: a fixture date after 2025 would be masked too.
Reverse: fix the clock in both engines and drop the masks.

## CU-C4-D4. Keep two upstream refusals as cases of their own

Decision: `annotation-copy.json` has `a-reversed-rect-refuses-the-copy` and `form-copy.json` has
`a-value-on-a-widget-left-out-is-refused`; the other copy cases use annotation 9 instead of 6, and
`copy-one-widget-of-a-group` copies without values (`2fba1ac5b`).
Reason: the first generator run refused four cases against their covers text. The causes are
upstream behaviour: annotation 6's `/Rect [200 740 180 720]` reads with `width: -20`, which the
page-space create schema refuses, and copying radio widget 7 with the group's value `Blue` names a
value only widget 8 has. Recording them under covers text that says so keeps both behaviours checked.
Cost: none.
Reverse: n/a.

## CU-C4-D5. The replay opens as `LocalEngine` does and reads the `Host`'s envelope

Decision: `tests/change_ops.rs` opens each fixture with `reserveObjectNumbers: 64`
(`OBJECT_NUMBER_POOL_SIZE`), drops the `Host`-only `events` and `mutationSequence` from a
`document.apply` answer, and compares a read whose payload is one `snapshot` or `list` with the value
itself (`dc1581ac5`, `a18e65790`, `d09e505e4`).
Reason: the TS session reserves 64 numbers at open, so its creates start at 75 and a copy may name 40.
The two envelope differences are the accepted events envelope and the `Host` answering
`pages.list`/`annotations.list` unwrapped, as every other runner reads them.
Cost: the open's reservation is asserted in the runner, not recorded in the vector.
Reverse: record the open request in the vector and send it as recorded.

## CU-C4-D6. Raw viewport handles close on drop

Decision: `pdfium::measure::RawViewport` wraps the raw handle, calls `EPDFViewport_Close` on drop and
lends a `Viewport` for the borrow; `features::measure::read_viewports` and `set_scale` take the
document and page index and go through the raw functions, as upstream's `MeasureReader` and
`MeasureMutator` now do (`3df062aec`, `dc1581ac5`).
Reason: the header makes a raw viewport its own origin, closing every measure handle with it; tying
the measure's lifetime to the borrow makes a use after close a compile error.
Cost: `set_scale` and `read_viewports` lost their `PageView` parameter (both are `epdf-engine`
internal; their one caller each moved).
Reverse: revert the two commits' `measure.rs` hunks.

## CU-C4-D7. `API_VERSION` is a constant checked against the vector

Decision: `features::changes::kinds::API_VERSION` is `"3.2"`, and the `kinds.json` replay asserts it
equals the vector's `apiVersion` (`53763b310`).
Reason: the TS derives it from `packages/engine/core/package.json`, which the Rust crates don't read.
Cost: a minor release that moves the TS version fails the test until the constant follows.
Reverse: read the version from the package file in a build script.

## CU-C4-D8. The page-box check applies to imports too

Decision: `plan_annotation.rs`'s shared `draft_of` refuses a page-space `rect` or `box` with a negative
or non-finite width or height (`PageBoxSchema`), for annotation imports as well as copies
(`d09e505e4`).
Reason: upstream checks both through the same `draftOf` and `AnnotationDraftSchema`. The Rust check
ran only after conversion to the file's coordinates, where a reversed `/Rect` no longer shows.
Cost: an import of a bundle row with a negative width is now refused with `InvalidArg`, as the TS
import refuses it; no existing vector has one.
Reverse: move the check into `plan_annotation_copy`.

## CU-C4-D9. The design import's widgets carry `dashArray` and `checkStyle`

Decision: `plan_form.rs`'s `placement_of`, shared by `forms.import` and `forms.copy`, writes a row's
`dashArray` and `checkStyle` when it has them (`dffca3290`).
Reason: `a390171cc` added both to the shared `widgetPlacementOf`, so the TS import writes them too.
Cost: none known; no vector reaches a widget with either.
Reverse: drop the two lines.

## CU-C4-D10. Copy limits travel on the change services

Decision: `ChangeServices` and `ChangeContext` carry `BundleLimits`. `document.applyChanges` reads its
optional `limits` (`5496eb084`), and every other change uses `DEFAULT_BUNDLE_LIMITS` (`d09e505e4`).
Reason: upstream's `ChangeApplier` takes the request's limits with the defaults as its fallback.
Cost: none.
Reverse: n/a.

## CU-C4-D11. An annotation copy reads its drawings and files through the export's collector

Decision: `prepare_annotation_copy` reads a stamp's drawing and an attachment's file through the
export's `ResourceCollector` (`d09e505e4`).
Reason: it already reads both from raw handles, once per shared drawing, and counts them against the
limits.
Cost: the collector keeps resources by content id, so two copies of one attachment count once against
`resources` and `bundleBytes`, where `AnnotationCopier` counts each copy's file. The answer differs
only when a copy is at those limits. Listed as a known gap; no vector reaches it.
Reverse: count files per copy in `prepare_annotation_copy`.

## CU-C4-D12. Rewrite only my own unshared commits

Decision: the vectors commit's message was amended (34 kinds to 41) and two engine commits were folded
into `dc1581ac5` so every commit builds.
Reason: both were local to this job's branch, before any bundle left the container.
Cost: none.
Reverse: n/a.
