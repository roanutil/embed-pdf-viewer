package com.embedpdf.scaffold.app

import android.app.Application
import android.graphics.Bitmap
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.embedpdf.scaffold.ArgbImage
import com.embedpdf.scaffold.EpdfDocument
import com.embedpdf.scaffold.EpdfEngine
import com.embedpdf.scaffold.OpsException
import com.embedpdf.scaffold.Size
import com.embedpdf.scaffold.renderPageArgb
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Field for field the same state as apps/ios/EpdfScaffold/DocumentModel.swift,
 * and now the same lifecycle too: no document ever ends up stored and
 * unclosed, whether the ViewModel is torn down mid-load or a load simply
 * fails. iOS gets there through @MainActor isolation; this gets there
 * through [DocumentLifecycle], driven only from viewModelScope's dispatcher
 * (Main) -- the `document` field itself was the bug (assigned from
 * Dispatchers.IO, read from onCleared() with no synchronization at all), so
 * this version never writes a document anywhere until it is back on Main
 * and DocumentLifecycle has confirmed the load is still current.
 */
class DocumentViewModel(app: Application) : AndroidViewModel(app) {
    var pageCount by mutableStateOf(0u)
        private set
    var sizeLabel by mutableStateOf("")
        private set
    var bitmap by mutableStateOf<Bitmap?>(null)
        private set
    var textPreview by mutableStateOf("")
        private set
    var searchHitCount by mutableStateOf(0)
        private set
    var failure by mutableStateOf<String?>(null)
        private set

    private val pageIndex = 4u

    // closeSync(), NOT close(): see the note on OpsException/EpdfDocument in
    // packages/kotlin/epdf-ops/src/main/kotlin/com/embedpdf/scaffold/Epdf.kt.
    // close() is uniffi's own non-blocking handle release; closeSync() is
    // epdf-uniffi's blocking one, which is the point of calling it here.
    private val lifecycle = DocumentLifecycle<EpdfDocument> { it.closeSync() }

    init {
        load()
    }

    private fun load() = viewModelScope.launch {
        val token = lifecycle.startLoad()
        var opened: EpdfDocument? = null
        try {
            val loaded = withContext(Dispatchers.IO) {
                val bytes = getApplication<Application>().assets.open("report.pdf").use { it.readBytes() }
                val doc = EpdfEngine().open(bytes, null)
                opened = doc

                LoadedPage(
                    document = doc,
                    pageCount = doc.pageCount(),
                    size = doc.pageSize(pageIndex),
                    text = doc.pageText(pageIndex),
                    searchHitCount = doc.search(pageIndex, "the", false).size,
                    image = doc.renderPageArgb(pageIndex, 2.0f),
                )
            }

            if (lifecycle.commit(token, loaded.document)) {
                pageCount = loaded.pageCount
                sizeLabel = "%.1f x %.1f pt".format(loaded.size.width, loaded.size.height)
                textPreview = loaded.text.take(600)
                searchHitCount = loaded.searchHitCount
                bitmap = Bitmap.createBitmap(
                    loaded.image.pixels,
                    loaded.image.width,
                    loaded.image.height,
                    Bitmap.Config.ARGB_8888,
                )
            }
        } catch (cancellation: CancellationException) {
            // Rethrown, not folded into `failure` below: swallowing it here
            // would tell the coroutine machinery this load finished
            // normally, which breaks structured concurrency for whatever
            // cancelled it. The document this load opened, if any, was never
            // committed to `lifecycle`, so it is this catch's job to close it.
            opened?.closeSync()
            throw cancellation
        } catch (error: Throwable) {
            opened?.closeSync()
            if (!lifecycle.isStale(token)) {
                failure = mapOpsError(error)
            }
        }
    }

    override fun onCleared() {
        lifecycle.close()
    }
}

/** Everything one load produces, held locally until [DocumentLifecycle.commit] agrees it should be stored. */
private data class LoadedPage(
    val document: EpdfDocument,
    val pageCount: UInt,
    val size: Size,
    val text: String,
    val searchHitCount: Int,
    val image: ArgbImage,
)

/**
 * NOT error.message for an OpsException: uniffi's Kotlin generator renders
 * every variant's `message` as a field dump, so EngineFailed and Internal
 * read "detail=..." rather than the human sentence. Their `detail` property
 * holds the raw text. See packages/kotlin/README.md.
 *
 * Standalone (no Android/ViewModel dependency) so it can be exercised by a
 * JVM test without a device or the native library: OpsException instances
 * construct directly.
 */
fun mapOpsError(error: Throwable): String = when (error) {
    is OpsException.EngineFailed -> error.detail
    is OpsException.Internal -> error.detail
    else -> error.toString()
}
