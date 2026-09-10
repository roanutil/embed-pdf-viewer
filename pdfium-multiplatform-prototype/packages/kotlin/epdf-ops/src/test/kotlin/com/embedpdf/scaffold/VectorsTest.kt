package com.embedpdf.scaffold

import java.io.File
import kotlin.math.abs
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.test.assertFailsWith
import org.json.JSONObject

/**
 * Reads pdfium-multiplatform-prototype/vectors/report-page4.json, the same frozen file the
 * Rust, Swift and JS suites read. `epdf.scaffold.root` is set by the Gradle
 * test task.
 */
private val root = File(System.getProperty("epdf.scaffold.root"))
private val vectors = JSONObject(File(root, "vectors/report-page4.json").readText())
private val render = vectors.getJSONObject("render")
private val golden = File(root, "vectors/${render.getString("golden")}").readBytes()
private val fixture = File(root, "fixtures/report.pdf").readBytes()

private fun meanAbsDiff(a: ByteArray, b: ByteArray): Double {
    assertEquals(a.size, b.size, "buffer lengths differ")
    var total = 0L
    for (i in a.indices) {
        total += abs((a[i].toInt() and 0xFF) - (b[i].toInt() and 0xFF)).toLong()
    }
    return total.toDouble() / a.size
}

/**
 * Largest single-byte absolute difference. `meanAbsDiff` averages over the
 * whole buffer, so one badly wrong region can hide under a mean-based
 * tolerance as long as most of the buffer matches. This bounds the worst
 * byte instead, mirroring test-support's max_abs_diff on the Rust side and
 * the Swift and JS suites' maxAbsDiff.
 */
private fun maxAbsDiff(a: ByteArray, b: ByteArray): Int {
    assertEquals(a.size, b.size, "buffer lengths differ")
    var worst = 0
    for (i in a.indices) {
        worst = maxOf(worst, abs((a[i].toInt() and 0xFF) - (b[i].toInt() and 0xFF)))
    }
    return worst
}

class VectorsTest {
    private fun open(): EpdfDocument = EpdfEngine().open(fixture, null)

    @Test
    fun matchesTheFrozenVectors() {
        val pageIndex = vectors.getInt("pageIndex").toUInt()
        val doc = open()
        try {
            assertEquals(vectors.getInt("pageCount").toUInt(), doc.pageCount())

            val size = doc.pageSize(pageIndex)
            val want = vectors.getJSONObject("size")
            assertTrue(abs(size.width - want.getDouble("width").toFloat()) < 0.001f)
            assertTrue(abs(size.height - want.getDouble("height").toFloat()) < 0.001f)

            val text = doc.pageText(pageIndex)
            assertEquals(vectors.getString("text"), text)
            assertEquals(vectors.getInt("charCount"), text.length)

            val search = vectors.getJSONObject("search")
            val hits = doc.search(pageIndex, search.getString("query"), search.getBoolean("caseSensitive"))
            assertEquals(search.getInt("hitCount"), hits.size)

            val bitmap = doc.renderPage(pageIndex, render.getDouble("scale").toFloat())
            assertEquals(render.getInt("width").toUInt(), bitmap.width)
            assertEquals(render.getInt("height").toUInt(), bitmap.height)
            assertEquals(render.getInt("stride").toUInt(), bitmap.stride)
            val diff = meanAbsDiff(bitmap.bgra, golden)
            assertTrue(diff <= render.getDouble("meanAbsDiffTolerance"), "mean abs diff $diff")
            val worst = maxAbsDiff(bitmap.bgra, golden)
            assertTrue(worst <= render.getInt("maxAbsDiffTolerance"), "max abs diff $worst")
        } finally {
            doc.closeSync()
        }
    }

    @Test
    fun aPagePastTheEndThrows() {
        val doc = open()
        try {
            assertFailsWith<OpsException.PageOutOfRange> { doc.pageSize(doc.pageCount()) }
        } finally {
            doc.closeSync()
        }
    }

    @Test
    fun garbageBytesThrow() {
        assertFailsWith<OpsException.LoadFailed> {
            EpdfEngine().open("not a pdf".toByteArray(), null)
        }
    }

    @Test
    fun rendersAnArgbIntArray() {
        val pageIndex = vectors.getInt("pageIndex").toUInt()
        val doc = open()
        try {
            val image = doc.renderPageArgb(pageIndex, render.getDouble("scale").toFloat())
            assertEquals(render.getInt("width"), image.width)
            assertEquals(render.getInt("height"), image.height)
            assertEquals(image.width * image.height, image.pixels.size)
        } finally {
            doc.closeSync()
        }
    }

    /**
     * The frozen golden is grayscale (B == G == R in every pixel), so it
     * cannot catch a channel-order bug in `bgraToArgb`: a B<->R swap still
     * produces a mean and max abs diff of 0 against it. This test uses a
     * synthetic two-pixel buffer with distinct channel values instead, and
     * asserts the packed ints directly against ARGB values built from the
     * same raw components, not by calling `bgraToArgb` a second time.
     */
    @Test
    fun bgraToArgbPacksChannelsCorrectly() {
        // Pixel 0: B=10, G=20, R=30, A=255. Pixel 1: B=200, G=150, R=100, A=128.
        val bgra = byteArrayOf(
            10.toByte(), 20.toByte(), 30.toByte(), 255.toByte(),
            200.toByte(), 150.toByte(), 100.toByte(), 128.toByte(),
        )

        val pixels = bgraToArgb(bgra, width = 2, height = 1, stride = 8)

        assertEquals((255 shl 24) or (30 shl 16) or (20 shl 8) or 10, pixels[0])
        assertEquals((128 shl 24) or (100 shl 16) or (150 shl 8) or 200, pixels[1])
    }

    @Test
    fun rendersExactDeviceRGBColors() {
        val vector = JSONObject(File(root, "vectors/colors.json").readText())
        EpdfEngine().use { engine ->
            engine.open(File(root, "fixtures/colors.pdf").readBytes(), null).use { doc ->
                val bitmap = doc.renderPage(0u, vector.getDouble("scale").toFloat())
                assertEquals(vector.getInt("width").toUInt(), bitmap.width)
                assertEquals(vector.getInt("height").toUInt(), bitmap.height)
                val samples = vector.getJSONArray("samples")
                for (i in 0 until samples.length()) {
                    val sample = samples.getJSONObject(i)
                    val offset = sample.getInt("y") * bitmap.stride.toInt() + sample.getInt("x") * 4
                    val bgra = sample.getJSONArray("bgra")
                    for (channel in 0..3) {
                        assertEquals(bgra.getInt(channel), bitmap.bgra[offset + channel].toInt() and 255)
                    }
                }
            }
        }
    }
}
