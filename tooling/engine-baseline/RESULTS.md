# pages.geometry through the service boundary

Recorded 2026-09-22 at `41b45182f`, from
`results/geometry-service-boundary.json`. Seven rounds of twenty iterations per
arm per fixture, interleaved, arm order rotated by round, answer gate passed
first. 140 samples behind each cell.

Toolchain: Node v24.20.0, rustc 1.98.0 (88d9e12ae), emcc 4.0.0 (97c7c2adab), wasm
`35eb2b3e2ba3` (the `wasm32-eh` module with the Rust core linked in). Machine:
Apple silicon laptop, which is why these are a regression baseline and not a
device budget.

| Fixture                        | Glyphs | TypeScript median / p95 | Rust median / p95 | Ratio | Heap per snapshot |
| ------------------------------ | ------ | ----------------------- | ----------------- | ----- | ----------------- |
| `report.pdf` p4                | 8,106  | 11.052 / 12.907 ms      | 8.411 / 9.552 ms  | 1.31x | 2019 / 2017 KiB   |
| `report.pdf` p0                | 5,087  | 6.917 / 7.441 ms        | 5.524 / 6.105 ms  | 1.25x | 1292 / 1290 KiB   |
| `embedded_orientations.pdf` p0 | 80     | 0.131 / 0.149 ms        | 0.156 / 0.169 ms  | 0.84x | 40 / 39 KiB       |

Ratio is TypeScript median over Rust median, so above 1 means Rust is faster.

## What it says

Rust wins on dense pages and loses on tiny ones, and the crossover is the whole
result. The dense pages are where per-glyph boundary crossings cost something:
`PageGeometryReader` calls `EPDFText_GetCharGeometry` once per glyph across the
WASM boundary, 8,106 times for page 4, while the Rust reader crosses once and
loops on the other side. On an 80-glyph page there is nothing to amortize and
the fixed cost of encoding and parsing the envelope is the only thing left, so
Rust is 0.84x.

1.31x is a long way below the 2.6x that Section 3 records for per-item against
coarse in the binding benchmark, which is what that section warns about: those
arms isolate a crossing-cost benefit, and this one pays for the envelope, the
decode and the materialization as well. The plan says the committed 2.6x "says
a win exists, not how large this one must be". It exists, and it is 1.31x.

**Allocation is a wash.** 2017 KiB against 2019 KiB on the densest page. Both
arms end up building the same `PageGeometrySnapshot` object graph in the JS
heap, so the Rust arm's 1.7 MB of intermediate JSON does not show up as
retained memory. It shows up in the time column instead, which is where the
crossover on small pages comes from.

## What it does not say

**Transfer is not counted.** `InlineTransport` has no thread boundary, and
Section 10 asks for transfer to be in the measurement. A browser number needs
`tooling/viewer-e2e/` and a real worker. Until then these are decode and
materialize without the structured clone.

**There is no optimized TypeScript arm.** Section 9 requires one before any
release-optimization comparison. Nothing here argues that Rust is faster than
TypeScript could be made; it is a baseline for regression.

**This is one machine, warm, on one document.** The fixture corpus is three
pages from two files. Section 9's list of what the corpus still needs, rotated
and cropped pages, RTL and combining text, large scanned documents, is not
covered here.

## Setting the gate

Section 10 wants two numbers from this: no regression against the recorded
baseline, and an absolute latency ceiling in milliseconds.

The regression number should be read off the p95 column rather than the median,
since the median hides exactly the tail a viewer notices. A tolerance under
about 10% would fire on this machine's own noise: across the seven rounds the
TypeScript median for `report.pdf` p4 moved between 10.8 and 11.1 ms on two
runs half an hour apart.

The absolute ceiling is a product question and is not set here. What this says
is what it costs today: a caller asking for the densest page in the corpus
waits about 11 ms on the shipping reader and about 8.4 ms on the Rust one, on a
current laptop, with the document already open.
