# CU-C3 decisions

Decisions made while merging the impl catch-up into plugins and regenerating the conformance vectors
on `research/catchup-2026-11-12-plugins`. Each names the commit that carries it; "the docs commit" is
the one that adds this file.

## CU-C3-D1. Take plugins' `AGENT-BRIEF.md` in the merge

Decision: the add/add conflict in `docs/research/upstream-catchup-2026-11-07/AGENT-BRIEF.md` resolves
to plugins' copy (`3aa47e708`).
Reason: plugins' copy is impl's copy plus the "Phase 2" section, line for line (`diff` of the two shows
only that section).
Cost: none.
Reverse: `git checkout a7c75f5ad -- docs/research/upstream-catchup-2026-11-07/AGENT-BRIEF.md`.

## CU-C3-D2. Keep plugins' `.gitignore`

Decision: `3aa47e708` keeps plugins' `.gitignore`, though it merged without a conflict.
Reason: impl's `e4c17f48d` adds the Swift and Xcode block plugins already has, so the clean merge
listed it twice.
Cost: none.
Reverse: take the merged file from `git merge-tree`.

## CU-C3-D3. Rewrite the measurement suite against the new scale contract

Decision: `c74ec3e9d` rewrites `src/suites/measurement.ts` and its fixtures. Pages carry their
viewports on the layout, and a new `scaleEngine` builds each change's answer: `pages.setScale` items
and `pages.scaleSet` events with the layout from `viewportsWithScale`, then the re-measured
annotations. `viewport-reads-fail-refresh-and-rehydrate` and `refresh-epochs` go, replaced by
`scale-events-move-the-layout`; the deferred-read part of `create-measurement` goes; the function
cases for `clearLoadError` and `pageScaleOf` go. `service-writes-and-foreign-scales` no longer defers
its first answer, and `page-removed-while-a-change-waits` now waits on a deferred annotation load.
Reason: upstream `c7f8d75ff` and `8bc6546e5` removed `page.measure.listViewports` reads, the viewport
mirror, its load errors and the per-page write queue, so the old answers were never asked for (the
generator stopped with "unanswered engine calls"). Changes are now sent at once, so an answer deferred
past a later one would reach the plugin out of the engine's order.
Cost: coverage of read failures, read epochs and queue order is gone with the behaviour; the
measurement evidence (CU-C3-D6) has to be re-authored.
Reverse: none; the old cases describe code that no longer exists.

## CU-C3-D4. Follow the other API changes in the generator, not around them

Decision: in `c74ec3e9d`, the two `annotation-api` remeasure cases call `whenRemeasurable` and
`stageRemeasure` (`8bc6546e5`); `fixed-bytes-stamp-tool` removes both registrations so the tool still
goes (`f0bb7b047`); the `commands` covers text states the order and once-per-store-change asking
(`7af98f4df`). The fake engine lists `pages.setScale`, `annotations.copy` and `forms.copy`.
Reason: without them the cases recorded `"remeasurePage" is not a function`, or answers that
contradicted their covers text.
Cost: the remeasure report's `failed` list is always empty now; a refusal shows as a rollback in
the next `list`.
Reverse: revert `c74ec3e9d` and regenerate.

## CU-C3-D5. Re-baseline the evidence archives by re-capture and re-pin only

Decision: `b8ade7fe8` re-captures the archives whose capture scripts exist (c2 to c11), re-pins
`currentOwners` and source rows only where the row named the path's last owner at `7990560d6`, and
moves every integrity, preservation and family pin of a changed file to its new sha256,
transitively. Rows that pin older commits or removed files stay as they are. Witness assertions
change only where the upstream commit changed the outcome (`stageRemeasure`, refusal index, tool
registry throws, the import's `opId`).
Reason: the brief asks to re-baseline pins that trace to this regeneration; historical rows still
verify against their own commits.
Cost: the preservation pins of `test/annotation-live.test.ts` and `test/annotation-live-refusals.ts`
move, so those files are no longer the bytes the original migration preserved.
Reverse: revert `b8ade7fe8`.

## CU-C3-D6. Leave the measurement B3 evidence to CU-C5

Decision: `test/measurement.test.ts` (15 failures) and `stamp-placement`'s B3 aggregate (1 failure:
227 current cases now 222) stay failing.
Reason: these are behavioural witnesses of the viewport mirror, read epochs and per-page queue that
`8bc6546e5` removed, and a B3 re-accounting of every measurement mapping. That is evidence
authoring for the measurement area, which CU-C5 ports, not a pin re-baseline.
Cost: CU-C5 starts with 16 failing evidence tests in 2 files; 661 of 677 pass.
Reverse: re-author them here instead (about 1 to 2 hours).

## CU-C3-D7. List coverage gaps, add no cases

Decision: no new generator cases for upstream behaviour no vector reaches; the gaps are listed per
package in `results/CU-C3-coverage-gaps.md`, and the new engine behaviour without engine cases is
listed for CU-C4 in the report.
Reason: the brief says the port jobs add the cases.
Cost: `coverage:check` passes against a regenerated `coverage.json` that records the gaps as they are.
Reverse: not applicable.

## CU-C3-D8. Correct CU-C2's ledger rows, not its decision file

Decision: the ledger's impl TS row reads 96 of 98 and cites CU-C2-D7 (finding R2), and the ledger
names the 5 font kinds `operations:check` reported on impl (finding R3). `decisions/CU-C2.md` is not
edited.
Reason: the findings were routed here; a job's decision file is its own record.
Cost: CU-C2-D5 still says 4 kinds; the ledger carries the correction.
Reverse: edit the ledger rows back.
