# Platform package and conformance obligations

This addendum is part of the approved catch-up plan. The user explicitly reaffirmed keeping platform packages current and maintaining/expanding TS/Rust parity vectors during implementation.

## Acceptance rules

1. Every changed shared record/method/setting/event must update existing platform package bindings, facades, hosts and actual consumers that expose it. Match Rust source, runtime, headers, generated records, packaged assets, fonts and checksums; no stale package may be called current.
2. Each behavioral change gets a TS-oracle scenario and a Rust replay/invariant at its narrow owner. Expand generators/cases/coverage declarations when existing vectors omit new behavior; do not count default-valued fields or typechecks as parity.
3. Regenerate affected vectors against frozen-target merged TypeScript and matching runtime; inspect the semantic diff, check deterministic output and execute Rust replays. Preserve strict-write/tolerant-read, omitted/null/empty/value, cancellation/refusal, stale-answer and lifecycle cases.
4. Contract drift must exercise literal router methods and serialized payloads across actual platform boundaries. Regenerate UniFFI/WASM/C metadata and update the consuming package in the same reviewed task.
5. Final platform status includes real commands/results and matching source/artifact identities. Unavailable native hosts/devices are explicit incomplete gates; they are never inferred passes. Publication remains separately scoped.

## Package matrix

| Package surface | Must stay current | Required gate |
|---|---|---|
| Runtime/C and Rust/C ABI | Fork runtime graph, new callable symbols, headers, generated metadata, LinuxARM64 and selected slices | Native symbol/load/behavior checks plus source/build provenance |
| Apple Swift/iOS/macOS | Root and platform SwiftPM manifests, UniFFI records, facades, XCFramework checksum/slices, tiled UI lifetime | Package generation/compile and actual supported Apple consumers on macOS/Xcode |
| Android Kotlin | Generated records, facades/host callbacks, configured AAR/runtime packaging and16KB alignment | Generate/compile with NDK-capable host and available device smoke |
| WASM/browser | Generated exports, JS/router methods, payload/loader and package exports | TS/Rust vector replays, real browser snapshot/copy/save/user-activation and package consumption |
| .NET | Existing facade DTOs/helpers, loader/native package, current low-level rendering semantics | Matching local package relocation/consume/native raster graph; no implied new plugin facade or NuGet publication |

Jobs3–4 own semantic/vector/binding changes; Job5 owns final packaged consumer acceptance. Shared builds, geometry and DTO migration are counted once. Expand this matrix when an affected existing package is discovered, and record its owner in IMPLEMENTATION.md.
