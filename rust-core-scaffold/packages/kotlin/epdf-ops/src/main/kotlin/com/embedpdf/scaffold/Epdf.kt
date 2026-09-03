package com.embedpdf.scaffold

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
    val out = IntArray(width * height)

    for (y in 0 until height) {
        var source = y * stride
        var target = y * width
        for (x in 0 until width) {
            val b = bitmap.bgra[source].toInt() and 0xFF
            val g = bitmap.bgra[source + 1].toInt() and 0xFF
            val r = bitmap.bgra[source + 2].toInt() and 0xFF
            val a = bitmap.bgra[source + 3].toInt() and 0xFF
            out[target] = (a shl 24) or (r shl 16) or (g shl 8) or b
            source += 4
            target += 1
        }
    }
    return ArgbImage(out, width, height)
}
