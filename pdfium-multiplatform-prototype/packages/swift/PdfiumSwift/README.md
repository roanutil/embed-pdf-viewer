# Direct Swift PDFium implementation

This Swift 6.4 package implements the same six coarse operations as
`epdf-ops`, directly against PDFium's C API. It does not call Rust or UniFFI.
An engine owns one serial queue; PDFium initialization, handles, operations,
and destruction remain confined to that queue.

From the prototype root, compare release performance and executable size:

```bash
bash build/bench-swift-vs-rust.sh 20
```

The runner checks operation checksums before printing timings. Both binaries
load the same PDFium dylib. The size table counts the release executables;
Swift's system runtime and the common PDFium dylib are excluded.
