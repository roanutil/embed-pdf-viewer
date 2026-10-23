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
