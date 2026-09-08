# Additional conformance fixtures

`colors.pdf` is a repository-authored 80 × 20-point page containing four adjacent
20 × 20 DeviceRGB rectangles: red, green, blue, and RGB (0.2, 0.4, 0.6).
`vectors/colors.json` records exact interior BGRA samples at scale 1; these are
specified colors, not output captured from the implementation under test. Sampling
away from edges avoids cross-platform antialiasing differences.

`encrypted-hello.pdf` is copied from PDFium's
`testing/resources/encrypted_hello_world_r6.pdf` at fork commit `7f0fa5dbe`.
It is covered by `PDFium-LICENSE`. The UTF-8 user password is `hôtel`, as documented
in `core/fpdfapi/parser/cpdf_security_handler_embeddertest.cpp`. Tests cover absent,
incorrect, and correct passwords against the same fixture.
