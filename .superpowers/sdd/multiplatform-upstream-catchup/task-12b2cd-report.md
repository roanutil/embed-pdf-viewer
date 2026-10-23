# Task 12b2c + 12b2d

Implementation base: `107ac347bd93d2b81fa332cebbc94c12e78175b2`. Combined review base: `e4f60a19960b68c0e7ec60631bf30d102dbccf33` (includes previously committed C11).

Final gates, from `tooling/plugin-conformance`:

- `pnpm exec tsc --noEmit` — exit 0, zero diagnostics (initially 32).
- `pnpm exec tsx --conditions=development src/staleness.ts` — exit 0; `stale: 112`, zero `no-import`, zero `throws` (initially 4 no-import and 3 throws). Stale vectors remain for 12c; no generation performed.
- `node --import=tsx --conditions=development --test test/harness.test.ts test/stamp-current.test.ts test/form-current.test.ts test/form-script.test.ts test/form-live.test.ts test/remaining-current.test.ts` — 111 tests passed, zero failures/skips.
- `git diff --check` — clean.

Material decisions:

- Migrated remaining private registrations to `create`/`connect`, fixed discovery exports, and isolated legacy stamp controller imports with the existing scoped module host. Legacy stamp asset-engine inputs use the existing typed local engine adapter; settings now reach the current controller. No shipping/helper implementation was changed.
- Render uses current refs, layer read rights, policy service, field/annotation ledgers and fact payload paths. Closing cancels pending tile work. Removed the reconstructed reducer and removed paint-version API cases; direct current invalidation transitions retain content/annotation/field/no-op coverage.
- Signature generators use current signing keys, service methods, field authority arguments, DTOs, nested stamp APIs and confirmed preparation/completion/cancellation events. A concrete current stamp creation answer covers the now-exercised annotation fallback. The removed capability-only constructor is replaced by connected-controller coverage. Current model transitions replace removed reducer cases.
- UI retains schema normalization, solver, projection and shortcut cases. Removed chrome bundle/global transform cases are replaced by current typed layout creation/validation, immutable add/replace, scoped remove, identity, warnings and edit-chain cases. Kernel record comparison inputs now exercise shipping `shallowEqual`; removed origin extraction cases have no fabricated substitute.
- Removed only three obsolete active `form-fixtures.ts` byte guards from existing tests; retained useful behavior tests and existing historical files/assertions. Added four focused behavioral tests. No new archives, digest inventories or preservation guards.

No vectors, Rust, shipping packages or `.github` changes. `crates/plugins/acrojs/prelude.js` restored to HEAD after the final importing command and before commit. The controller-owned top-level plan amendment is excluded from staging.

Ready for the combined independent review, then directly 12c. The intentional remaining limitation is stale vectors.

## Authorized legacy-fixture removal and review repair

Review-fix base: `1d65d8deaf14867b6fb167a41d27d24eb4c95239`.

The first bounded repeatability check exposed construction and input defects in addition to the legacy namespace mismatch: spreading the workspace context read its throwing document getter, the workshop answered obsolete local-engine APIs, and free placement used obsolete coordinates. That approach stopped at the requested failed-verification threshold. The user then authorized completing the migration rather than keeping the legacy wrappers.

- Migrated every signature scenario to existing `annotation-live`, `form-current`, and `stamp-current` factories and helpers. Kept every case name. Current form snapshots include typed widget records in page coordinates; stamp calls serialize bytes, arm through the document view, and await asset work before consuming it. Raster creation uses existing `seedDoc`/`appendDoc`; imports author the correct signature-library catalog write; placements use current `center` and typed `apply` confirmations. Confirmations describe the actual placed geometry and annotation identity.
- Deleted `annotation-fixtures.ts`, `form-fixtures.ts`, and `stamp-fixtures.ts`. With no legacy query consumers left, dropped the proposed namespace expansion: `stamp-module-host.ts` is unchanged from the review base.
- Kept sorted-discovery/host-identity behavior in `stamp-current.test.ts` while removing its deleted legacy import. `link-current.test.ts` already imports `annotation-live-fixtures`; its only legacy mention is a negative source assertion, so no change was necessary there.
- Added complete-outcome repeatability plus explicit successful asset-placement settlement and exact engine mutation assertions. Added checks that imported signature marks reach field requests and report the intended refused fill. Updated the armed-case descriptions to reflect current annotation-hit precedence and the current gesture event.
- Retired only obsolete active-source dependencies: C1's obsolete deferred-consumer freeze; C5's obsolete whole-tree active-byte preservation loop; measurement's active stamp/signature source freezes. Kept the historical files, archived assertions, and useful behavior tests. No new archives, digests, guards, vector generation, or shipping changes.

Validation from `tooling/plugin-conformance`:

- `pnpm exec tsc --noEmit` — exit 0, zero TypeScript diagnostics (including after the test-guard edits).
- `node --import=tsx --conditions=development --test test/remaining-current.test.ts` — 6 passed, zero failed/skipped. Both repeated placement runs create `stamp-lib-0-0:Logo`, send the expected page-space mutation and resolve placement of annotation 60; complete results match.
- `node --import=tsx --conditions=development --test --test-name-pattern='full sorted discovery|outermost stamp|caller cancellation|viewer cache|in-flight requests' test/stamp-current.test.ts test/link-current.test.ts` — 5 passed, zero failed/skipped. Covers descriptor restoration, sorted discovery, and current link behavior without running archive suites.
- `node --import=tsx --conditions=development --test --test-name-pattern='shared scripted exports|command registry|original definitions, occurrence accounting|complete original B3 archive' test/acrojs-current.test.ts test/commands-current.test.ts test/measurement.test.ts` — 4 passed, zero failed/skipped; the targeted deletion-consumer checks and command behavior run without file-not-found errors. No full historical audit was run.
- `pnpm exec tsx --conditions=development src/staleness.ts` — final exit 0: `stale: 112`; zero `no-import`, zero `throws`. The sandbox initially refused tsx's local IPC socket; the approved rerun completed. The first migration run found the signature-library PieceInfo answer mismatch; the current helper's explicit `catalogWrites` resolved it.
- A direct `node --import=tsx --conditions=development --input-type=module` signature-only audit ran every scenario from `signature-plugin.ts`, asserting no unmatched engine calls and successful construction — passed. Named adversarial errors remain intentional. Compared all case-name declarations against HEAD: unchanged.
- `git diff --check` — clean.

Existing environment notices: pnpm cannot interpolate the unset `NPM_TOKEN`; Node reports the existing `module.register()` deprecation. Neither caused a test or compiler failure. The prelude is restored after the last importing command and before the explicit tooling commit; the root-owned plan is excluded. Ready for one scoped re-review, then 12c; stale vectors remain intentional.
