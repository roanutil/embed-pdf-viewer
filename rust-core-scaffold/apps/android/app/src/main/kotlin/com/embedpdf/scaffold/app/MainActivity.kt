package com.embedpdf.scaffold.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp

class MainActivity : ComponentActivity() {
    private val model: DocumentViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                Column(
                    Modifier.verticalScroll(rememberScrollState()).padding(16.dp)
                ) {
                    model.failure?.let {
                        Text(it, color = MaterialTheme.colorScheme.error, fontFamily = FontFamily.Monospace)
                    }
                    Text("Pages: ${model.pageCount}")
                    Text("Page 4: ${model.sizeLabel}")
                    Text("Hits for \"the\": ${model.searchHitCount}")
                    model.bitmap?.let {
                        Image(it.asImageBitmap(), contentDescription = "page 4", modifier = Modifier.fillMaxWidth())
                    }
                    Text(model.textPreview, fontFamily = FontFamily.Monospace)
                }
            }
        }
    }
}
