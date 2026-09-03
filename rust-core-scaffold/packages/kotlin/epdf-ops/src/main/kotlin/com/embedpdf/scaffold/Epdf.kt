package com.embedpdf.scaffold

/**
 * `EpdfDocument` has two ways to shut a document down, and they are not
 * equivalent. Both are declared in the generated
 * `generated/com/embedpdf/scaffold/epdf_uniffi.kt`, not here, so this note
 * lives in the hand-written file next to them instead.
 *
 * `close()` is uniffi's own. Every uniffi Kotlin object implements
 * `Disposable` and `AutoCloseable`, and the generator always emits its own
 * `close()` for that: it frees the FFI handle and, on the Rust side, drops
 * `epdf_ops::Document`. `Drop` calls the non-blocking `drop_close()`
 * (crates/epdf-ops/src/engine.rs), which sets the closed flag immediately
 * but only enqueues the removal on the worker thread. It does not wait for
 * that removal to run.
 *
 * `closeSync()` is epdf-uniffi's own `EpdfDocument.close`, renamed for
 * Kotlin by `uniffi.toml` because a class cannot declare two methods
 * named `close()`. It blocks until the removal has actually run.
 *
 * `use { doc -> ... }` calls `Closeable.close()`, the non-blocking path, not
 * `closeSync()`. Nothing leaks and nothing is skipped either way, the
 * worker still processes the removal, but `use` alone gives up the
 * deterministic-cleanup guarantee `closeSync()` exists for. Prefer an
 * explicit `closeSync()` in a `finally` when that determinism matters:
 *
 * ```
 * val doc = engine.open(bytes, password = null)
 * try {
 *     // ...
 * } finally {
 *     doc.closeSync()
 * }
 * ```
 */

/**
 * Reading `OpsException` text: `.message` vs `.detail`.
 *
 * uniffi's Kotlin generator gives every `OpsException` variant its own
 * `override val message`, built by formatting that variant's fields, not by
 * copying the `thiserror` message text from crates/epdf-uniffi/src/lib.rs.
 * For `EngineFailed` and `Internal` specifically, that field was renamed to
 * `detail` (also in `uniffi.toml`, to dodge a separate `message` collision),
 * and `.message` renders as `"detail=<value>"` -- e.g. an `EngineFailed`
 * whose `detail` is `"pdfium reinit failed"` has `.message` equal to the
 * literal string `"detail=pdfium reinit failed"`, not the thiserror sentence
 * "the PDF engine failed and must be recreated: pdfium reinit failed".
 * Nothing is lost: the raw string is right there in `.detail`.
 *
 * `.message` is fine for a debug log. For `EngineFailed` and `Internal`,
 * read `.detail` for text meant to be shown to a person.
 */

/** ARGB pixels with the dimensions epdf-ops actually rendered at. */
data class ArgbImage(val pixels: IntArray, val width: Int, val height: Int)

/**
 * The whole Kotlin-side addition to what uniffi generates: BGRA bytes to an
 * ARGB IntArray, which is what android.graphics.Bitmap.createBitmap and
 * Compose's ImageBitmap both want.
 *
 * It returns the width and height rather than letting the caller recompute
 * them from `page_size * scale`. A second rounding rule is the one thing Task 5
 * warns against: epdf-ops uses max(1, round(points * scale)) and nothing else
 * may guess at it.
 */
fun EpdfDocument.renderPageArgb(index: UInt, scale: Float): ArgbImage {
    val bitmap = renderPage(index, scale)
    val width = bitmap.width.toInt()
    val height = bitmap.height.toInt()
    val stride = bitmap.stride.toInt()
    return ArgbImage(bgraToArgb(bitmap.bgra, width, height, stride), width, height)
}

/**
 * Packs a BGRA buffer, byte order B,G,R,A per pixel, into ARGB ints. Split
 * out of `renderPageArgb` so this channel logic can be exercised with a
 * synthetic buffer, not only through a real PDFium render; see
 * `bgraToArgbPacksChannelsCorrectly` in VectorsTest.kt.
 */
fun bgraToArgb(bgra: ByteArray, width: Int, height: Int, stride: Int): IntArray {
    val out = IntArray(width * height)

    for (y in 0 until height) {
        var source = y * stride
        var target = y * width
        for (x in 0 until width) {
            val b = bgra[source].toInt() and 0xFF
            val g = bgra[source + 1].toInt() and 0xFF
            val r = bgra[source + 2].toInt() and 0xFF
            val a = bgra[source + 3].toInt() and 0xFF
            out[target] = (a shl 24) or (r shl 16) or (g shl 8) or b
            source += 4
            target += 1
        }
    }
    return out
}
