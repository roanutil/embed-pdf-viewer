# Swift parity integration

Source: `research/swift-api-plugin-parity-plan` at `b7229a03a`.
Target before merge: `research/multi-platform-rust-core-impl-plugins` at `9ec4bff36`.

## Merge adjustments

The Git merge was clean, but the source branch predates the November Rust API
catch-up. The integration adapts attachment operations to Rust create/delete,
preserves attachment sizeBytes/md5, decodes event origins with subject/at,
transaction/serverSequence, and reads history settings changedKeys.

The regression tests first failed on the old origin decoder and attachment
metadata round trip, then passed after these changes. The refreshed inventory
covers 31 crates and 790 members: 605 mappings and 185 explicit exclusions.
Eleven existing i18n mappings previously misclassified as exclusions are now
checked. New Rust snapshot members are explicitly tracked as an unexposed
workflow, rather than being mistaken for full native coverage.

## Core-only branch

No incoming change is independently applicable to
`research/multi-platform-rust-core-impl`. That branch has the engine-level
generated Swift package, but neither epdf-ffi nor the plugin ViewerHost/Swift
facade. The revision-stream improvement and shared event-origin records belong
to that plugin host. The core-only branch is intentionally left unchanged.

## Validation

- Affected Rust suites: 320 tests passed, including real native host
  history/outline/attachment and saved-document workflows.
- Affected Rust clippy with all targets and warnings denied: passed.
- Inventory checker: 31 crates, 790 members accounted for.
- Inventory checker regression tests: 3 passed.
- Four-slice Apple rebuild and generated graph comparison: passed.
- macOS arm64 source-engine package: 355 API, 41 UI/model and 8 binding
  smoke tests passed (404 total).
- iOS 26.5 simulator source-engine package: the same 404 tests passed.
- Fresh independent integration review: no findings.

Rust host tests use the local fork 3c6e02297 static macOS runtime under
`perf-compare/.work/fork/`; the older default darwin-arm64 header cache lacks
epdf_annot.h and is not a valid input for this source tree.

## Remaining work

The user selected app-facing APIs for further coverage. The reviewed design is
`docs/superpowers/specs/2026-11-09-swift-app-api-coverage-design.md`. It covers
core document controls, host fetch, live settings, command controls, annotation
and form editing, snapshots, and extending the audit beyond plugin members.
Rust-owned caches, painter objects, and internal host plumbing remain private.

The downloaded engine pin predates the new exports. Generated source-engine
bindings must be rebuilt for this tree; a matching published engine and pin
update are required before claiming compatibility with the default downloaded
package. Source integration does not publish a runtime release.
