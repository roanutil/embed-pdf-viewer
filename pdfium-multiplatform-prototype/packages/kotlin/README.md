# Kotlin prototype bindings

UniFFI generates Kotlin bindings from
[`crates/epdf-uniffi`](../../crates/epdf-uniffi/src/lib.rs).
The handwritten [Epdf.kt](epdf-ops/src/main/kotlin/com/embedpdf/scaffold/Epdf.kt)
converts BGRA bytes into packed ARGB integers and returns them with the rendered
dimensions. The Android app uses those integers to create an Android bitmap.

Build from the prototype root, then run the host JVM tests:

```bash
bash build/build-kotlin.sh
(cd packages/kotlin && ./gradlew :epdf-ops:test)
```

The default build targets ARM64 macOS. Set `JAVA_HOME` to a suitable installed
JDK; the build script defaults to Android Studio's bundled JDK on macOS.
Gradle requests a Java 21 toolchain for compilation. See the
[Android app instructions](../../apps/android/README.md) for mobile builds.

## Closing documents

`EpdfDocument.close()` releases the generated wrapper's reference to Rust.
Once the last Rust document reference is dropped, the native core queues removal
of the PDFium document without waiting for it to finish. It does not guarantee
that removal has completed when `close()` returns.

`closeSync()` is the prototype's explicit document close, renamed by
[`uniffi.toml`](uniffi.toml). On the first close of a live document, it waits for
the worker's removal operation if the worker remains available. It does not
release the generated Kotlin wrapper. Repeated or concurrent close calls are
not an additional synchronization barrier.

Use `use` to release wrappers. When you also need to wait for document removal,
call `closeSync()` inside that scope:

```kotlin
EpdfEngine().use { engine ->
    engine.open(bytes, password = null).use { doc ->
        try {
            println(doc.pageCount())
        } finally {
            doc.closeSync()
        }
    }
}
```

Reuse an engine for multiple documents when appropriate. UniFFI's `use` helper
releases the wrapper; it does not invoke the renamed `closeSync()` operation.
See [UniFFI's Kotlin lifetime guidance](https://mozilla.github.io/uniffi-rs/latest/kotlin/lifetimes.html).

## Error text

[`uniffi.toml`](uniffi.toml) renames the Rust `message` fields of `EngineFailed`
and `Internal` to `detail`, avoiding a collision with Kotlin exception
`message`. Handle errors by variant. Treat `.message` and `.detail` as diagnostic
text; map them to an application-specific message for display.

## Tests

[VectorsTest.kt](epdf-ops/src/test/kotlin/com/embedpdf/scaffold/VectorsTest.kt)
checks report output, invalid input, page bounds, ARGB conversion, exact color
samples, and the UTF-16 length of extracted text. These are host JVM tests;
they do not test an Android device or Compose rendering.
