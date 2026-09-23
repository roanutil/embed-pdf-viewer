package com.embedpdf

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.epdf_uniffi.PdfEngine
import uniffi.epdf_uniffi.PdfException

/**
 * The Phase 1 Android smoke run.
 *
 * Instrumented rather than local, and the assertions are a deliberate copy of
 * `crates/epdf-uniffi/tests/native_surface.rs`. What only a device can say is
 * that the AAR's `libepdf_uniffi.so` loads next to `libc++_shared.so` under
 * Android's linker, against the fork's own PDFium build. Identical assertions
 * mean a divergence is a packaging problem and nothing else.
 *
 * The emulator this is meant to run on is a 16 KB page-size image, which is
 * the configuration Android 15 requires and the one a 4 KB-aligned `.so`
 * fails to load on.
 */
@RunWith(AndroidJUnit4::class)
class SmokeTest {
    private fun fixture(name: String): ByteArray =
        InstrumentationRegistry.getInstrumentation().context.assets.open("$name.pdf").use {
            it.readBytes()
        }

    @Test
    fun aDocumentOpensAndReportsItsPages() {
        val engine = PdfEngine()
        try {
            val document = engine.open(fixture("report"), null)
            val pages = document.pages()

            assertEquals(14, pages.size)
            assertEquals(0, pages[0].index.toInt())
            assertTrue("an index is not identity", pages[0].objectNumber > 0u)
            assertTrue(pages[0].widthPoints > 0f && pages[0].heightPoints > 0f)
        } finally {
            engine.shutdown()
        }
    }

    @Test
    fun aMissingPasswordReportsACodeAFacadeCanPromptOn() {
        val engine = PdfEngine()
        try {
            val refused = assertThrows(PdfException.Engine::class.java) {
                engine.open(fixture("encrypted-hello"), null)
            }
            assertEquals("DocPasswordRequired", refused.code)
        } finally {
            engine.shutdown()
        }
    }

    @Test
    fun geometryComesBackAsTheSameSnapshotTheWebEngineProduces() {
        val engine = PdfEngine()
        try {
            val document = engine.open(fixture("report"), null)
            val json = JSONObject(document.pageGeometryJson(document.pages()[0].objectNumber))
            val runs = json.getJSONArray("runs")
            var glyphs = 0
            for (i in 0 until runs.length()) {
                glyphs += runs.getJSONObject(i).getJSONArray("glyphs").length()
            }

            // The counts the frozen corpus vectors carry for this page.
            assertEquals(238, runs.length())
            assertEquals(5087, glyphs)
        } finally {
            engine.shutdown()
        }
    }

    @Test
    fun aPageRendersToRgbaTheCallerOwns() {
        val engine = PdfEngine()
        try {
            val document = engine.open(fixture("colors"), null)
            val bitmap = document.renderPage(document.pages()[0].objectNumber, 1.0f)

            assertTrue(bitmap.width > 0u && bitmap.height > 0u)
            assertTrue(
                "stride is explicit because it may exceed the packed row width",
                bitmap.stride >= bitmap.width * 4u,
            )
            assertEquals((bitmap.stride * bitmap.height).toInt(), bitmap.rgba.size)

            // Channel order, on a fixture with saturated interior colors. A
            // grayscale page cannot tell a correct conversion from a B-to-R
            // swap. Kotlin's Byte is signed, so compare as unsigned.
            var sawRed = false
            var sawBlue = false
            for (row in 0 until bitmap.height.toInt()) {
                val start = row * bitmap.stride.toInt()
                for (x in 0 until bitmap.width.toInt() * 4 step 4) {
                    val r = bitmap.rgba[start + x].toUByte().toInt()
                    val g = bitmap.rgba[start + x + 1].toUByte().toInt()
                    val b = bitmap.rgba[start + x + 2].toUByte().toInt()
                    if (r > 200 && g < 80 && b < 80) sawRed = true
                    if (b > 200 && g < 80 && r < 80) sawBlue = true
                }
            }
            assertTrue("no red pixel: the channels are probably swapped", sawRed)
            assertTrue("no blue pixel: the channels are probably swapped", sawBlue)

            // The bytes are the caller's, so the image outlives its document.
            val before = bitmap.rgba.copyOf()
            document.shutdown()
            assertArrayEquals(before, bitmap.rgba)
        } finally {
            engine.shutdown()
        }
    }

    @Test
    fun aBadScaleIsRefusedBeforeAnythingIsAllocated() {
        val engine = PdfEngine()
        try {
            val document = engine.open(fixture("report"), null)
            val number = document.pages()[0].objectNumber
            for (scale in listOf(0f, -1f, Float.NaN, Float.POSITIVE_INFINITY)) {
                val refused = assertThrows(PdfException.Engine::class.java) {
                    document.renderPage(number, scale)
                }
                assertEquals("scale $scale should be refused", "InvalidArg", refused.code)
            }
        } finally {
            engine.shutdown()
        }
    }

    @Test
    fun closingTheEngineClosesItsDocumentsAndRefusesNewWork() {
        val engine = PdfEngine()
        val document = engine.open(fixture("report"), null)
        document.pages()

        engine.shutdown()
        assertTrue(engine.isClosed())
        assertEquals(
            "DocNotOpen",
            assertThrows(PdfException.Engine::class.java) { document.pages() }.code,
        )
        assertEquals(
            "DocNotOpen",
            assertThrows(PdfException.Engine::class.java) {
                engine.open(fixture("report"), null)
            }.code,
        )

        // Idempotent, per the lifecycle contract.
        engine.shutdown()
    }

    @Test
    fun everyEngineCallRunsOffTheCallersThread() {
        // The property the whole design exists for. A Kotlin dispatcher
        // reserves the right to resume a coroutine on a different thread, so
        // confinement cannot be something the caller promises. Calling from
        // several threads at once would be a data race if any PDFium work ran
        // on the caller.
        val engine = PdfEngine()
        try {
            val document = engine.open(fixture("report"), null)
            val done = CountDownLatch(4)
            val counts = IntArray(4)
            for (i in 0 until 4) {
                Thread {
                    counts[i] = document.pages().size
                    done.countDown()
                }.start()
            }
            assertTrue(done.await(60, TimeUnit.SECONDS))
            assertTrue(counts.all { it == 14 })
        } finally {
            engine.shutdown()
        }
    }

    @Test
    fun twoEnginesAreIndependent() {
        val first = PdfEngine()
        val second = PdfEngine()
        val a = first.open(fixture("report"), null)
        val b = second.open(fixture("report"), null)

        first.shutdown()
        assertThrows(PdfException.Engine::class.java) { a.pages() }
        assertEquals("the other engine still works", 14, b.pages().size)
        second.shutdown()
    }
}
