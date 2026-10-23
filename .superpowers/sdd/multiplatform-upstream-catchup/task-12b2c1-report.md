# Task12b2c1: AcroJS foundation

Status: DONE_WITH_CONCERNS, ready for controller independent review. BASE `79b16091b9950935e5df77966f4207a1eddcce07`. Changes are private conformance tooling and this report only. No package/Rust/vector/.github/ledger changes, pushes, merges or shared harness redesign. The known import-generated prelude change was inspected and restored after the final import.

## Result and shipping evidence

`src/suites/acrojs.ts` now constructs the probe through actual `definePlugin.create`, returns `{api}`, and owns the shipping host with `ctx.cleanup`. This remains a workspace probe without state, sibling registrations or a `connect` hook; it directly invokes the shipping per-document host algorithm, rather than emulating it. Explicit `dispose` remains public for the original lifecycle cases. `ctx.cleanup` is independently exercised by kernel destroy. Driver operations/world/config have current TypeScript types before entering the shipping host.

The two formerly malformed field snapshots are complete `FormSnapshot`s built using reviewed, read-only current fixtures. Current refs use `objectNumber`; readOnly/required are flat; field widget relationships supply toggle export values. Scalar/array/none/unsupported entries, Off toggles, option projection (including stripping the original extra property), readOnly/required and empty fields now return exact independently asserted values. The original sixth bogus entry was never a valid `FormValueEntry`; that invalid component is archived and explicitly retired in favor of a typed unsupported-entry witness. No whole case is retired.

Identity arguments are current typed `Identity` values. `user_id`/`display_name` move to userId/displayName; corporation now uses organization rather than the old group-id policy. Missing optional claims use undefined; the existing explicit null override still independently exercises nullish fallback. Source evidence is the actual `c78997346647b0e53f7c8d1b98b80865f05a299e` host diff, `c3b90c3a4c22c547d45fca2fe0b4de5936478241` flag diff and `5da4ca52a` snapshot/widget diff, committed as `upstream-*.out`. Current owners: `packages/core/acrojs/src/host.ts:91-239`, `input.ts:10-71`, `packages/engine/core/src/forms/value-entry.ts:2-6`, and kernel `types.ts` create/returned API/cleanup contract. The inherited narrow security-only document test double and scripted unknown-answer boundary remain; no new suite cast, blanket any, suppression or old API restoration was introduced.

## Accounting and preservation

All evidence paths below are under `tooling/plugin-conformance/historical/task-12b2c1/` unless stated otherwise.

| Measure | Frozen original | Current |
|---|---:|---:|
| Pure cases | 75 | 75 |
| Host scenarios | 17 | 20 |
| Total occurrences | 92 | 95 |
| Original occurrences mapped | 92 | 92 |
| Exact original inputs retained / migrated | — | 85 / 7 |
| Whole retirements / additions | — | 0 / 3 |
| Pure throws | 6 | 4 named incomplete-tree negatives |
| Construction refusals | 17 | 0 |
| Executed scenario steps | 0 | 62 |
| Unexpected step throws / unmatched / reports | — | 0 / 0 / 0 |
| Settlements / expected rejections | — | 53 / 9 |
| Callbacks / console callbacks | — | 79 / 0 |

The seven migrated inputs are the two field snapshots and five identity cases; `identity-no-claims` is unchanged. All 17 original scenario inputs remain exact. Three additions are `kernel-destroy-owns-host`, `dispose-pending-factory`, and `sandbox-output-is-returned`. All current case names are unique. `original-acrojs.ts.txt`, `original-inputs.json`, `original-pure-outcomes.json`, `accounting.json`, `source-evidence.json`, and `current-inputs-runtime.json` preserve full source, occurrence identity, original/current hashes, complete inputs, exact outcomes and the malformed-entry partial retirement. Originals came from the frozen C evidence; no complete C recapture was performed.

The entire shared AnswerSpec/scriptedCallback/SandboxSpec/scriptedSandboxFactory block is byte-identical to the archived source. `deferred-inputs.json` freezes full actions160/link29/stage300/stage-plugin82/signature-plugin58 inputs and source hashes, plus legacy stamp/form bridge source hashes. The focused tests compare complete current constructed inputs and unchanged source bytes. Reviewed helper/scenario/fake/vector/entropy files are unchanged. The single covering preservation run passed all 403 existing tests, including reviewed annotation149 families, form221 families, stage and B3 stamp227 families and their existing archives/accounting checks; none of their expectations were changed.

## Semantic checks and every negative channel

Ten focused tests independently assert every host scenario's complete settlements, callback order and full default sandbox inputs, exact boot source arrays, boot/run event changes, deterministic sequence, all budgets, optional document/page/annotation planes, field results and identity results. Deferred transactions/factories have no early run or settlement. Queue failure does not poison later transactions; boot output goes to one caller, auto-boot caches its output once, no-source boot does not call the sandbox, failed source reads degrade and retry on rebuild, resource faults rebuild/reboot with the expected cache lifetime, and explicit/repeated/kernel disposal releases observed resources. Nonempty sandbox output preserves rc/event edits/form effects/UI effects/diagnostics exactly. A direct shipping-host test proves plain Promise/no abort, admitted work completion, and refusal of new transactions after disposal.

Pure negative names are exactly `sources-incomplete-throws`, `sources-incomplete-is-truthiness`, `sources-incomplete-string`, and `program-incomplete-throws`, all pinned to their archived shipping error outcomes. The other original pure outcomes are unchanged except the explicitly migrated fields/identity cases, which have literal current expected results. Both former missing-widgets TypeErrors are eliminated.

Settled rejections are exactly: `transactions-serialize` → body broke; `sandbox-run-throws` → vm exploded; `sandbox-boot-throws` → boot exploded; `boot-sources-throw` → no catalog; `factory-rejects` → no vm twice; `dispose` and `dispose-before-any-realm` → script host is disposed; `dispose-with-a-rejected-factory` → no vm. Error name/message/channel are asserted literally. The eight scenarios produce nine rejections. Source is `host.ts` queue/boot/run/factory/disposal plus the explicitly scripted host answers/body throw.

Output diagnostic channels are exactly: `boots-once-and-hands-the-output-to-one-caller` → script-error/boot#0; `boot-sources-read-fails` → script-error/Document boot script failed (continuing without it): catalog unreadable; `sandbox-output-is-returned` → script-error/script refusal. They appear only inside the corresponding resolved transaction output; all other returned/settled output data are asserted exactly. Callback inputs contain no diagnostic reports; only named sandbox/source callbacks occur, with exact ordering and data. Construction exceptions, executed step exceptions, runner exceptions, reports, console callbacks, engine ops/unmatched ops, state/document events, unexpected synchronous returned data, unmatched settlements and dangling promises are zero. No default-VM diagnostic is relabeled as successful scripted coverage.

## Commands and evidence

Commands run from `tooling/plugin-conformance` unless otherwise stated. Logs are committed in the evidence directory.

1. `node --import=tsx --conditions=development --test test/acrojs-current.test.ts`: final exit0, 10/10 pass (`focused-tests.out`). Initial RED reproduced host construction and malformed snapshot failures (`red.out`); the lifecycle-addition RED is `lifetime-red.out`. One initial repository-root invocation could not resolve tsx; package-local execution is the correct command. No failure is an accepted positive outcome.
2. `node --import=tsx --conditions=development test/acrojs-evidence.ts`: exit0; renders `core/acrojs.json`75 and `core/acrojs-host.json`20 with `emit(...,'render')`, without writing vectors (`runtime-render.out`). Writes only private historical accounting/runtime evidence.
3. `./node_modules/.bin/tsc --noEmit --target ES2022 --module ESNext --moduleResolution bundler --strict --skipLibCheck --lib ES2022,DOM --types node --customConditions development src/suites/acrojs.ts test/acrojs-current.test.ts test/acrojs-evidence.ts`: exit0, no diagnostics (`owned-typecheck.out`). An initial implicit evidence-array type was corrected with an explicit record/outcome union before the final check.
4. `./node_modules/.bin/tsc --noEmit`: exit2, exactly56 unowned diagnostics (`full-typecheck.out`), down from57 by the one owned AcroJS diagnostic. Allocation: actions4, annotation bridge3, commands1, form bridge12, i18n12, link1, render-pure2, render2, shell6, signature-core2, signature-plugin1, stamp bridge10. Thus31 remaining C +25 unchanged bridges. No global compiler-clean claim.
5. `node --import=tsx --conditions=development --test test/harness.test.ts test/annotation-current-pure.test.ts test/annotation-live.test.ts test/annotation-live-ii.test.ts test/form-current.test.ts test/form-live.test.ts test/form-script.test.ts test/stamp-current.test.ts test/stamp-mutations.test.ts test/stamp-placement.test.ts test/measurement.test.ts test/current-suites.test.ts`: exit0,403/403 pass,0 skipped/cancelled (`preservation-tests.out`). One covering run only; no broad capture rerun. `git diff --check` is clean. Only the inspected known prelude side effect was restored; `prelude-import.diff` and `prelude-restoration.json` retain generated/head/restored hashes, restored to `be5e1652ba1496826f2cc2ddd85be94e8d9f53e025a91399877e739f87d9609a`. Node26's loader deprecation is environmental.

## Task11, Task13 and carries

Binding `task-11-cases.out` lists both AcroJS paths at inputDiff0 (75 pure/17 host, lost0/added0). Neither belongs to the31 inherited Task11 drift cases. The seven current input migrations are fresh Task12 changes, fully mapped; no inherited drift is silently treated as repaired.

Task13 reconstructs the serialized current function args before invocation: complete fields/widgets/calculationOrder/FormSnapshot flags/refs/value-entry unions; current Identity userId/displayName/organization and the explicit null override; every actual ScriptWorldInput/event/page/annotation option. The probe uses the script-host token, workspace scope, create returning api, no state/connect work, and cleanup owning the real host. Its config includes typed document/identity/budget plus the unchanged scripted source/factory answer tables. Transactions carry ordered boot/run/wait/throw op arrays; boot/run accept native world and optional full budget. Environment nowMs1000/offset0 and host sequence seedFrom(doc-1,sequence) are recorded before sandbox invocation, including after rebuild. The scripted factory/boot/run/dispose callbacks and exact sandbox returned event/effects/diagnostics are the external ports. Deferred labels remain slow/vm; no engine fact/apply-op schema is introduced here, and no old action/field adapter is added. Promise IDs, settlement turns, diagnostic channels, boot-once/pending-output ownership and cleanup must remain reproducible. There is no new fake abort port.

Two explicit concerns remain. The default QuickJS loader gap is unchanged: these cases test the actual shipping host over scripted sandboxes, not the default VM loader. Also, current `host.ts:130-133,216-239` permits an admitted transaction to build a second sandbox after disposal of its pending first factory; the added case pins exact factory/source/dispose/run ordering, completion and a repeated disposal releasing the second resource. Controller ruled this is current behavior to preserve and a Task13/Task14 carry, not cancellation or a no-new-resource-after-dispose guarantee. No shipping fix was made. D still owns the separate global compiler/no-import/runtime/diagnostic closure and legacy bridge/shim retirement.

Next controller action: independent review of this commit and `task-12b2c1-report.md`; C2 remains undispatched here.
