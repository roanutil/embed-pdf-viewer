# ``EmbedPDF``

Open, render, search, and edit PDF documents using a native engine and configurable plugins.

## Overview

`EmbedPDF` supplies a document workspace and typed capabilities. Your application supplies its file access, UI, and persistence. Use `EmbedPDFUI` for SwiftUI page images, a scrolling reader, and observable models, or present the capabilities through your own interface.

Start with <doc:GettingStarted> to render a page. Enable only the plugins your application needs, acquire their capabilities from ``PDFViewer``, and handle failures using ``PDFError``. The package currently declares iOS 17 or later; its checked-in manifest requires Swift tools 6.2, and the pinned prebuilt engine requires Xcode 27.1 or later.

The Swift facade delegates to a Rust workspace kernel and a native PDFium engine. Async operations do not require your application to manage engine threads. UI state still belongs on the main actor.

## Topics

### Getting started

- <doc:GettingStarted>
- <doc:ConfiguringPlugins>
- <doc:DocumentsAndLifetime>
- <doc:CoordinatesAndPages>
- <doc:ErrorsAndObservation>

### Task guides

- <doc:SearchingAndRendering>
- <doc:EditingAndSaving>
- <doc:ExtendingTheViewer>

### Workspace and documents

- ``PDFViewer``
- ``ViewerConfig``
- ``DocumentInfo``
- ``DocumentStatus``
- ``DocumentEvent``

### Page geometry

- ``PageInfo``
- ``PageRef``
- ``Point``
- ``Rect``
- ``TextQuad``

### Rendering and navigation

- ``RenderCapability``
- ``RenderedPage``
- ``StageCapability``
- ``SearchCapability``
- ``PageRenderResolution``

### Selection and document content

- ``SelectionCapability``
- ``LinkCapability``
- ``MetadataCapability``
- ``AnnotationCapability``
- ``FormCapability``

### Editing and signing

- ``RedactionCapability``
- ``MeasurementCapability``
- ``StampCapability``
- ``PageEditCapability``
- ``SignatureCapability``

### Application integration

- ``InteractionCapability``
- ``FeedbackCapability``
- ``ActionsCapability``
- ``CommandsCapability``
- ``ViewManagerCapability``

### Presentation and host services

- ``ShellCapability``
- ``I18nCapability``
- ``JavaScriptCoreEngineFactory``
- ``KeychainSigner``
- ``Registration``

### Saving and failures

- ``SaveMode``
- ``PDFError``
- ``PluginErrorInfo``
- ``HostError``
- ``HostFailure``
