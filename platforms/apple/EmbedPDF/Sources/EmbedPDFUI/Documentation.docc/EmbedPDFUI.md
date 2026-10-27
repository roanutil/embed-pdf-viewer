# ``EmbedPDFUI``

Display PDF pages and connect SwiftUI controls to an EmbedPDF document.

## Overview

EmbedPDFUI supplies a scrolling and zooming page surface, individual page images,
a tiled image compositor, and observable reader and search models. The
`EmbedPDF` module owns viewer configuration, document access, and capabilities.
Your application owns installation, document lifetime, surrounding controls,
error presentation, and accessible document content.

Use ``StageReaderView`` to display the stage's visible pages with built-in drag,
pinch, and double-tap gestures. Use ``PageImageView`` for a page in your own
layout, or ``TiledPageImage`` when a custom reader needs detail at deep zoom.
These surfaces do not include an annotation toolbar, text selection, search
highlight overlays, or an accessible PDF text layer.

The package targets iOS 17 and macOS 14 and later and declares Swift tools 6.2.
EmbedPDFUI uses SwiftUI and Observation, with UIKit haptics on iOS and AppKit
haptics on macOS. macOS feedback requires compatible hardware and uses generic
feedback for impact and notification requests.
For package installation and viewer startup, open **Render your first PDF page**
in the `EmbedPDF` module's documentation in Xcode. The articles below begin with an already
started viewer and an open document, so you can add one UI task at a time.

## Topics

### Display a document

- <doc:DisplaySwiftUIReader>
- ``StageReaderView``
- ``StageReaderModel``
- ``PageImageView``

### Render detail at deep zoom

- <doc:RenderTiledPages>
- ``TiledPageImage``
- ``TiledPageModel``

### Search and manage UI lifetime

- <doc:AddSearchControls>
- ``SearchModel``
- ``HapticFeedback``
