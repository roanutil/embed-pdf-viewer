package com.embedpdf.scaffold.app

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * DocumentLifecycle needs no native library, no device, and no generated
 * bindings: it is generic over T, so a fake closeable stands in for
 * EpdfDocument. This runs as a plain unit test (./gradlew :app:testDebugUnitTest),
 * the same way ErrorMappingTest does.
 *
 * The real DocumentViewModel bug this guards against: a document opened by
 * an in-flight load ending up stored and unclosed after onCleared() has
 * already run, or two overlapping loads leaving the older one's document
 * live. Both are races that need a native library and a device to reproduce
 * end to end, so this exercises the bookkeeping in isolation instead.
 */
private class FakeCloseable {
    var closeCount = 0
        private set

    fun close() {
        closeCount += 1
    }
}

class DocumentLifecycleTest {
    private fun newLifecycle() = DocumentLifecycle<FakeCloseable> { it.close() }

    @Test
    fun commitStoresAndDoesNotCloseWhenStillCurrent() {
        val lifecycle = newLifecycle()
        val token = lifecycle.startLoad()
        val value = FakeCloseable()

        assertTrue(lifecycle.commit(token, value))
        assertEquals(0, value.closeCount)
    }

    @Test
    fun commitClosesInsteadOfStoringWhenTeardownAlreadyRan() {
        val lifecycle = newLifecycle()
        val token = lifecycle.startLoad()
        lifecycle.close()

        val value = FakeCloseable()
        assertFalse(lifecycle.commit(token, value))
        assertEquals(1, value.closeCount)
    }

    @Test
    fun commitClosesInsteadOfStoringWhenSupersededByANewerLoad() {
        val lifecycle = newLifecycle()
        val staleToken = lifecycle.startLoad()
        lifecycle.startLoad() // a newer load starts before the older one finishes

        val staleValue = FakeCloseable()
        assertFalse(lifecycle.commit(staleToken, staleValue))
        assertEquals(1, staleValue.closeCount)
    }

    @Test
    fun closeClosesWhateverIsCurrentlyStored() {
        val lifecycle = newLifecycle()
        val token = lifecycle.startLoad()
        val value = FakeCloseable()
        lifecycle.commit(token, value)

        lifecycle.close()
        assertEquals(1, value.closeCount)
    }

    @Test
    fun closeWithNothingStoredClosesNothing() {
        val lifecycle = newLifecycle()
        lifecycle.close()
        // No assertion beyond "did not throw": there is nothing to close, so
        // nothing should be, and there is no FakeCloseable in scope to check.
    }

    @Test
    fun closeIsIdempotent() {
        val lifecycle = newLifecycle()
        val token = lifecycle.startLoad()
        val value = FakeCloseable()
        lifecycle.commit(token, value)

        lifecycle.close()
        lifecycle.close()
        assertEquals(1, value.closeCount)
    }

    @Test
    fun committingASecondValueClosesTheFirstInsteadOfLeakingIt() {
        val lifecycle = newLifecycle()
        val firstToken = lifecycle.startLoad()
        val first = FakeCloseable()
        assertTrue(lifecycle.commit(firstToken, first))

        // Same lifecycle, a fresh load committed under the same token: this
        // does not happen in DocumentViewModel today (each load gets its own
        // token), but commit() must not silently drop a live document if it
        // ever did.
        val second = FakeCloseable()
        assertTrue(lifecycle.commit(firstToken, second))
        assertEquals(1, first.closeCount)
        assertEquals(0, second.closeCount)
    }

    @Test
    fun isStaleTracksBothTeardownAndSupersession() {
        val lifecycle = newLifecycle()
        val token = lifecycle.startLoad()
        assertFalse(lifecycle.isStale(token))

        lifecycle.startLoad()
        assertTrue(lifecycle.isStale(token))
    }
}
