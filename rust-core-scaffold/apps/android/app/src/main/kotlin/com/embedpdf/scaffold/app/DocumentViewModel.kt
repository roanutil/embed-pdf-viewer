package com.embedpdf.scaffold.app

import android.app.Application
import android.graphics.Bitmap
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.embedpdf.scaffold.EpdfDocument
import com.embedpdf.scaffold.EpdfEngine
import com.embedpdf.scaffold.OpsException
import com.embedpdf.scaffold.renderPageArgb
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Field for field the same state as apps/ios/EpdfScaffold/DocumentModel.swift. */
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
    private var document: EpdfDocument? = null

    init {
        load()
    }

    private fun load() = viewModelScope.launch {
        try {
            withContext(Dispatchers.IO) {
                val bytes = getApplication<Application>().assets.open("report.pdf").use { it.readBytes() }
                val doc = EpdfEngine().open(bytes, null)
                document = doc

                val count = doc.pageCount()
                val size = doc.pageSize(pageIndex)
                val text = doc.pageText(pageIndex)
                val hits = doc.search(pageIndex, "the", false).size
                val image = doc.renderPageArgb(pageIndex, 2.0f)
                val rendered = Bitmap.createBitmap(
                    image.pixels,
                    image.width,
                    image.height,
                    Bitmap.Config.ARGB_8888,
                )

                withContext(Dispatchers.Main) {
                    pageCount = count
                    sizeLabel = "%.1f x %.1f pt".format(size.width, size.height)
                    textPreview = text.take(600)
                    searchHitCount = hits
                    bitmap = rendered
                }
            }
        } catch (error: Throwable) {
            failure = mapOpsError(error)
        }
    }

    override fun onCleared() {
        // closeSync(), NOT close(). uniffi's Kotlin generator makes every
        // exported object implement AutoCloseable with its own
        // `override fun close()` that frees the FFI handle, which collides
        // with epdf-uniffi's exported `close`. packages/kotlin/uniffi.toml
        // renames ours to closeSync for Kotlin only; Swift still says close().
        document?.closeSync()
        document = null
    }
}

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
