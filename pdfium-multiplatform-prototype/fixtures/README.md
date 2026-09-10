# PDF fixtures

`report.pdf` matches the repository's
[example report](../../examples/snippet-react/public/report.pdf).
The report expectations are in
[`vectors/report-page4.json`](../vectors/report-page4.json); page index 4 means
the fifth page.

`colors.pdf` contains an 80 × 20-point page with four adjacent 20 × 20-point
DeviceRGB rectangles: red, green, blue, and RGB (0.2, 0.4, 0.6).
[`vectors/colors.json`](../vectors/colors.json) specifies interior BGRA samples
at scale 1. Sampling inside each rectangle avoids testing its antialiased edges.
These color checks can detect channel swaps that the grayscale report cannot.

`encrypted-hello.pdf` matches PDFium's
[`encrypted_hello_world_r6.pdf`](../../packages/engine/runtime/runtime-src/testing/resources/encrypted_hello_world_r6.pdf).
The accompanying [PDFium license](PDFium-LICENSE) is preserved unchanged.
The password is `hôtel`. Node and web tests in
[`additional-vectors.mjs`](../packages/test-support/additional-vectors.mjs)
check missing, incorrect, and correct passwords, including the accented character.
