# Final whole-change review

Reviewed base `35b18b786` through head `5e3236108968d84ac45666ccac14c3b2d03e3c3a` against the approved design and implementation plan. The separately verified parity merge is the base, not part of this verdict. Source/index/HEAD were not changed. The supplied 15,048-line review package was read in bounded chunks; truncated sections were recovered, and unchanged code was inspected only for concrete geometry and permission questions.

## Strengths

1. URL opening uses the kernel's actual fetch/source lifecycle. Failed HTTP/non-PDF responses retain retryable tabs, byte/layer sources regenerate per-attempt buffers, close cancels transport, and late completion cannot resurrect a tab. Real-host tests check buffer release and refetch counts.
2. Settings retain one Rust owner. Partial patches preserve omitted fields, explicit nullable clears cross the boundary, reset restores registered values, and tests prove command runners, script factories, identity providers, locale loaders, signer/trust ports, asset engines and annotation registrations survive scalar changes/reset.
3. Editing uses existing capability policy and confirmed-event reconciliation. Binary bundle resources, typed identities, independent widgets, calculation order, script permissions, malformed inputs, omitted/null fields and save/reopen are exercised through real hosts. Held-operation tests prove cancellation ends a caller's wait while admitted edits still reconcile late refusals.
4. Snapshot metadata precedes pixels, individual readers cancel independently, clear/close abort shared work, and returned pixels survive the viewer. Crop/absolute-rotation tests inspect dimensions, colors and scale; batch images now retain each render's admitted geometry without duplicating host release ownership.
5. The Swift facade remains thin, with typed records and explicitly permitted form/annotation domain JSON. The core-inclusive report distinguishes source API verification, native presentation seams, printing integration and the separate binary release requirement.

## Issues

### Critical — must fix

None.

### Important — should fix

1. **Snapshot scale can use geometry from before render admission.** `crates/plugins/render/src/ffi/snapshot.rs:61` captures `held.display_width` and passes it to `Raster::read` at line 63. That width comes from `crates/plugins/render/src/snapshot.rs:177`, before the image crosses the `changes.when_settled()` gate at lines 156–174. A page layout change or a pending geometry edit's refusal during that gate can shrink the crop used by `render_page`, leaving the old divisor. For a 2-point capture at scale 2, resized to a 1-point effective width before render admission, the returned 2-pixel image reports scale **1**, not **2**. Native placement/export consumers cannot trust its pixels-per-point contract.

   A temporary regression using the existing `Rig`, fake engine and freshly compiled library graph reproduces the mismatch. It dispatches a capture and then a page-layout event in the same kernel turn, verifies the actual render request is clipped to width 1, resolves a 2×2 raster, and asserts scale 2. Result: one test failed, actual 1 versus expected 2. This uses the deliberately yielding snapshot settlement gate; direct single-page render was checked separately and selects its geometry synchronously through inline `Promise::spawn`.

   Fix: read the resolved `PageImageHandle`'s per-render `point_width()` when constructing the snapshot raster, as the fixed batch path does. Preserve immediate capture metadata and shared cancellation ownership. Add a held/gated snapshot geometry regression, including a quarter-turn case.

   Exact reproduction command: `python3 /private/tmp/swift-final-snapshot-review/reproduce.py`. Inputs: `/private/tmp/swift-final-snapshot-review/regression.rs` and `support.rs`; evidence: `result.log` and `compile.log` in the same directory. The script compiles only this temporary test against existing cached libraries; it does not rebuild or mutate repository source. It returns exit 101 on the reviewed graph. The cached library paths are specific to this local validation environment.

### Minor — nice to have

1. **The overload audit cannot distinguish Swift argument labels.** `platforms/apple/check-swift-api-coverage.py:65` iterates overload mappings, but the Swift check at line 69 only matches the base name. Removing `create(draftJSON:)` while retaining `create(_:)` still satisfies both mappings; the equivalent problem affects `update(_:patchJSON:)` and `addWidget(to:placementJSON:)`. All required APIs currently exist, so this is audit sensitivity rather than missing functionality. Record/check distinguishing Swift labels and add a regression removing just the JSON overload. This confirms the Task 5 reviewer finding; its synthetic test was not repeated.
2. **The inventory excludes the delivered current-pages workflow.** `platforms/apple/swift-api-coverage.json:9316` still excludes `StageCapability.list_current_pages` as host presentation, although `crates/plugins/stage/src/ffi.rs:1075` and `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Stage.swift:98` expose it, and the zoomed-spread regression establishes its distinct behavior. Map these methods and refresh the report/README counts. For this correction alone: 724 mapped, 136 excluded, 860 total; plugin counts become 686 mapped/106 excluded and host-presentation exclusions become 64.
3. **Record and narrow the existing native warning noise.** `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Form.swift:10` retains a pre-existing public Foundation import that triggers `UnusedImportAccess` in the iOS log at line 1287. Narrow/remove that import. Xcode also emits three “Metadata extraction skipped, no AppIntents.framework dependency found” warnings at log lines 2107, 2583 and 2627; document these as tool-generated warnings rather than changing unrelated product capabilities to silence them. All native suites still pass. This is not a runtime blocker.

## Validation and review limits

Existing evidence was inspected, not rerun: the Rust log contains 127 successful result groups totaling **1,045 passed, 0 failed, 0 ignored**; clippy finishes successfully; the framework log records native graph verification and the four Apple architecture slices. macOS and iOS logs each show **378 API + 41 UI/model + 8 binding smoke = 427 passed**, with iOS ending `TEST SUCCEEDED`. The report records seven passing checker tests. The only new test execution was the concrete snapshot regression above, which exposes coverage missing from those green suites.

Permission-exclusion questions were checked narrowly: form `can_fill` delegates to `may_fill`, while annotation `is_editable` describes by-hand presentation settings rather than replacing write authority. No new policy implementation in Swift was found. Existing registration ports and plugin close guards were evaluated through their changed forwarding code and behavioral tests. No claim is made that declaration accounting proves every runtime condition.

## Declined to judge

1. Native printing implementation: explicitly deferred by the approved design until a real `HostPrinter` supplies prepared representations and disposal lifetime; current documentation truthfully says it is absent.
2. Published engine artifacts, checksums and package pins: explicitly a separate release operation; this review judges source/local-engine behavior, not default downloaded-binary usability.
3. Exporting private painter/cache resources, sibling scripting internals, text-editor layout and every web pointer/presentation seam: the selected app-facing contract retains these in Rust/native presentation rather than requiring public implementation-method parity.
4. Runtime reauthorization of already-open engine sessions and independent cancellation of retry: approved rulings preserve fixed session authority and close-driven retry cancellation, and the Swift docs state those limits.
5. The already pushed parity merge and transfer to the core-only branch: separate reviewed deliverable before this range; the ledger records no independent core-only fix to transfer.

## Recommendations

Complete one focused fix wave for the snapshot divisor, Swift overload labels, stale current-pages inventory entry and warning documentation/import. Reuse the newly discovered regression and update verification evidence for the changed boundaries. Do not restart broad behavioral review or conflate a source push with a runtime release.

## Assessment

**Ready to merge? With fixes.**

The implementation substantially meets the approved app-facing scope and preserves Rust ownership, permissions and cancellation. The confirmed snapshot geometry race is the only production blocker found; the other three findings concern accounting accuracy and validation-output clarity. After the focused fix and verification, the source branch can proceed to the authorized fork push.
