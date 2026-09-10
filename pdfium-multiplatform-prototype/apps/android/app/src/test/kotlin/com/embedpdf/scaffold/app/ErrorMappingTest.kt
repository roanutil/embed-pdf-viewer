package com.embedpdf.scaffold.app

import com.embedpdf.scaffold.OpsException
import kotlin.test.Test
import kotlin.test.assertEquals

/**
 * mapOpsError needs no native library and no device: OpsException instances
 * construct directly on the desktop JVM, so this runs as a plain unit test
 * (./gradlew :app:testDebugUnitTest).
 */
class ErrorMappingTest {
    @Test
    fun engineFailedUsesDetail() {
        val error = OpsException.EngineFailed("pdfium said no")
        assertEquals("pdfium said no", mapOpsError(error))
    }

    @Test
    fun internalUsesDetail() {
        val error = OpsException.Internal("unexpected null handle")
        assertEquals("unexpected null handle", mapOpsError(error))
    }

    @Test
    fun anythingElseFallsBackToToString() {
        val error = OpsException.PageOutOfRange(4u)
        assertEquals(error.toString(), mapOpsError(error))
    }
}
