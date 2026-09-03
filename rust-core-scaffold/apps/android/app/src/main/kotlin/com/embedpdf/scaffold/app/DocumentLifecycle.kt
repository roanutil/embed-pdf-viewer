package com.embedpdf.scaffold.app

/**
 * The latch-and-generation bookkeeping that keeps a document load from
 * storing its result once it no longer should: because teardown already
 * ran, or because a newer load has since superseded it. Mirrors the
 * invariant apps/ios/EpdfScaffold/DocumentModel.swift enforces (closed flag
 * + generation counter), not its mechanism: iOS gets thread confinement for
 * free from @MainActor isolation, so its fields are read and written
 * directly. This class holds no lock of its own and is only safe because
 * DocumentViewModel calls into it exclusively from viewModelScope's
 * dispatcher (Main) -- never from the Dispatchers.IO block that does the
 * actual FFI work. That confinement, not a lock, is what removes the data
 * race DocumentViewModel used to have on its `document` field.
 *
 * Generic over T (rather than hardcoding EpdfDocument) so it can be
 * exercised on a desktop JVM with a fake closeable, no native library and no
 * generated bindings required: see DocumentLifecycleTest.
 */
class DocumentLifecycle<T : Any>(private val closeValue: (T) -> Unit) {
    private var current: T? = null
    private var closed = false
    private var generation = 0

    /** Starts a new load. Returns the token to pass to [commit] or [isStale]. */
    fun startLoad(): Int {
        generation += 1
        return generation
    }

    /**
     * True when a load started with [token] is no longer the one this class
     * cares about: teardown already ran ([close]), or a later [startLoad]
     * superseded it. A caller whose background work failed or was cancelled
     * checks this before deciding the failure is still worth reporting.
     */
    fun isStale(token: Int): Boolean = closed || token != generation

    /**
     * Called once the load started with [token] has produced [value].
     * Stores it and returns true when the load is still current; otherwise
     * closes [value] itself -- it is never stored -- and returns false.
     */
    fun commit(token: Int, value: T): Boolean {
        if (isStale(token)) {
            closeValue(value)
            return false
        }
        current?.let(closeValue)
        current = value
        return true
    }

    /**
     * Tears down: closes whatever is currently stored, if anything, and
     * marks every future [commit] a no-op that closes its value instead of
     * storing it. Idempotent, so a second call is harmless.
     */
    fun close() {
        closed = true
        current?.let(closeValue)
        current = null
    }
}
