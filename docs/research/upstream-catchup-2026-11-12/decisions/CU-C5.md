# CU-C5 decisions

Decisions made while porting upstream `074a190c9..ed064e0c8` into the annotation and form family on
`research/catchup-2026-11-12-c5`. Each names the commit that carries it.

## CU-C5-D1. Make the kernel changes my crates need, each in its own commit

Decision: four changes to `crates/epdf-kernel`, which is CU-C6's crate: the page list read again on
`stream.desynced` (`c00f1d345`, upstream `a4805a492`), a staged change sent by an engine verb
(`bbd9bebf8`, `b3f1c447e`), `pages.setScale` prediction, `scaleBackOf` and `viewportsWithScale`
(`51a07da98`, `c7f8d75ff`), and copies naming their object numbers (`ece09927b`, `1481e4556`).
Reason: annotation hydration, form fill, signature, measurement and duplicate can't match their
vectors without them, and the brief allows the smallest change in its own commit. The manager was
told (kkt 1701, 1703).
Cost: if CU-C6 ports the same upstream commits, CU-C7 has to keep one copy of each.
Reverse: revert the four commits and take CU-C6's.

## CU-C5-D2. The engine verb's `send` gets no capability-scoped handle

Decision: `StagedChange::verb` takes `Fn(write) -> Promise<Value>`, and the annotation and form
imports call their own `doc` inside it (`bbd9bebf8`, `87726eae7`).
Reason: the Rust queue sends every change through one `apply` closure, with no per-capability
handle to pass the way `handleFor(capability)` does upstream.
Cost: none observable; both plugins' `doc` is the handle upstream passes.
Reverse: thread a handle through `DocumentChanges::send`.

## CU-C5-D3. Tell the annotation plugin a page's viewports when they differ by value

Decision: `connect_viewports` compares the list it last told by value; upstream compares the
array's identity (`dd57af5a4`).
Reason: Rust's page rows are JSON values with no identity to compare.
Cost: Rust skips telling an equal list again where upstream tells it. `setPageViewports` keeps the
last list, so the skipped calls change nothing.
Reverse: key the told lists on the registry's `Rc`.

## CU-C5-D4. Memoize a page's scale on the whole page list

Decision: `ScaleMemo` keys on the registry's `Rc` and the session's own viewports, as the previous
port keyed on the registry (`dd57af5a4`).
Reason: rows have no identity of their own; upstream keys on the layout object.
Cost: a page's `PageScale` is a new `Rc` whenever any page's layout changes, where upstream keeps
it while that page's layout object stays.
Reverse: give each row a stable identity in the kernel's page view.

## CU-C5-D5. Rewrite the measurement crate's Rust-only tests for one change per scale change

Decision: `tests/behavior.rs` drops the `pageScaleOf` test and rewrites the close and download
tests; `tests/ffi.rs` answers `apply` and publishes `pages.scaleSet` with the layout, and its
loading and failed-write tests become ready-at-open and a refused change failing every page
(`dd57af5a4`).
Reason: they exercised the viewport mirror and per-page queue that upstream `8bc6546e5` removed.
Cost: the per-page queue's ordering test is gone with the queue.
Reverse: none; the old tests describe code that no longer exists.

## CU-C5-D6. Re-account the measurement cases in a layer of its own

Decision: the B3 archives stay byte for byte. A new
`tooling/plugin-conformance/historical/catchup-2026-11-12-measurement/accounting.json` maps each
of the 64 cases task-12b2b3-i left current to its case now (36 unchanged, 22 rewritten, 6 retired
with reasons, 1 added), pinned to the old accounting's sha256. The B3 test and stamp-placement's
aggregate check the chain through it (`5dfee50b4`).
Reason: editing the pinned accounting would mean re-pinning the chained archives (CU-C3's
FEEDBACK), and the old rows still describe what B3 did.
Cost: one more file in the chain; a later catch-up that rewrites the suite adds another layer or
folds this one.
Reverse: revert `5dfee50b4`.

## CU-C5-D7. Rewritten evidence follows upstream, with each change named

Decision: the 15 measurement evidence tests take the new outcomes: no not-ready create, the queue's
"the document closed", the engine's "no page 1", `scaleError` codes `Unknown` (upstream serializes
the plugin error), and `apply` counts in place of `page.measure.setScale` (`5dfee50b4`).
Reason: each traces to `8bc6546e5` or `c7f8d75ff`; the comments in the test name which.
Cost: none beyond the review.
Reverse: revert `5dfee50b4`.

## CU-C5-D8. Port form field duplicate in this job, after annotation duplicate

Decision: annotation duplicate (`1481e4556`, `9eead5531`, `db8b5ffb1`, `f8ac5a351`) is ported first
with two vector cases (`ef5b91e19`..`f74c06c6d`), with an interim form-lender `copy_ops` that
refuses. Form field duplicate (`a390171cc`) follows in this job (`6c5a3b51d`..`d11e3e250`) and
replaces that refusal. An earlier draft deferred it to a follow-up; the manager ruled against
splitting (kkt 1711), since a separate job would edit the same crates.
Reason: the manager's ruling; a split job would conflict in `form`.
Cost: engine-core's `planFormCopies` and the `formFieldCreates` rules it reads now have a JSON copy
in `crates/plugins/form/src/engine/copy.rs`, beside `epdf-engine`'s typed one: two ports of one rule
to keep in step.
Reverse: not applicable.

## CU-C5-D9. Duplicate fixtures answer with the plugin's own copy rule

Decision: the two annotation-crud duplicate cases build their engine answers with the plugin's
`copiesOf` (engine-core `planAnnotationCopies`) plus the fresh `nm` an engine gives a copy
(`ef5b91e19`).
Reason: the fixtures construct answers explicitly, as the other annotation cases do; the `nm` makes
the answer visibly replace the prediction.
Cost: the answer agrees with the plugin's rule by construction, so the cases check what the plugin
does with an answer, not the engine's copy.
Reverse: answer with a real engine's copy result.

## CU-C5-D10. Two viewer-host failures are CU-C4's

Decision: `a_measurement_scale_persists_through_the_native_engine` and
`a_foreign_scale_is_read_and_kept_through_the_native_engine` in `epdf-viewer-host` stay failing.
Reason: the measurement plugin now writes scales as `apply` with `pages.setScale` and reads
`PageLayout.viewports`; `epdf-engine` refuses `unknown variant pages.setScale`. That op and the
layout viewports are CU-C4's port of `c7f8d75ff`.
Cost: 2 red tests until CU-C4's work is integrated.
Reverse: not applicable.

## CU-C5-D11. List the remaining coverage gaps rather than chase each line

Decision: added cases for the tool remover (`26227a437`) and duplicate; the measurement plugin's
no-service branches (`viewportsOf`, `writeLocal`, the `whenSettled` path) and its aborted-signal
line stay without a case, as do the copy picture-source lines in `read/render.ts` and
`selection.duplicate()` over a selection that holds a form's widgets in design mode (the lent path
through the form's `copyOps`, which `duplicateWidgets` covers on the form's side).
Reason: the no-service branches are the defensive ones the crate README already lists (no vector
reaches them; `tests/ffi.rs` `without_a_service_the_scale_is_in_memory` covers them in Rust), and a
stamp copy's preview needs a pending stamp placement plus a render, a case of its own.
Cost: those lines stay unreached by vectors.
Reverse: add the cases.

## CU-C5-D12. List new cases as catch-up additions in the live evidence and move the pins

Decision: annotation-live, annotation-live-ii, form-live and form-script count every case of their
suites. The four cases this job added are listed in those tests as catch-up additions after the
migration archives, with the upstream commit each covers, and the new copy write and refusals are
checked. Editing `annotation-live.test.ts` and `annotation-live-refusals.ts` moved their
preservation and integrity pins; CU-C3's `scripts/cu-c3/chainpins.py`, pointed at this worktree,
moved them through the chain (`af56c7bb4`; log `/workspaces/logs/CU-C5-chainpins.log`).
Reason: as CU-C3-D5, the archives pin the test files' bytes, and the new cases are new upstream
behaviour, not changes to what the archives recorded.
Cost: those two test files are no longer the bytes the original migration preserved, a second
step past CU-C3's.
Reverse: revert `af56c7bb4`.
