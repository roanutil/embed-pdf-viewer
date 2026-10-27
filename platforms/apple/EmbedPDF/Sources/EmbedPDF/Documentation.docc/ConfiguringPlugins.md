# Configure plugins and access

Choose the capabilities your workspace provides and the operations its documents may perform.

## Start with a small configuration

``ViewerConfig`` defaults to no optional plugins. `nil` plugin configurations and `false` flags leave those plugins out. Acquire capabilities after starting the viewer and opening a ready document.

```swift
import EmbedPDF

func makeReaderWorkspace() throws -> PDFViewer {
  try PDFViewer(ViewerConfig(
    metadata: true,
    search: SearchConfig(),
    render: RenderConfig(),
    stage: StageConfig()
  ))
}
```

Await `start()` on the returned viewer before using it. Rendering alone does not require stage, and search works without stage; installing stage lets search reveal hits in a viewport. This configuration supplies engine capabilities, not a toolbar or search field.

## Satisfy plugin dependencies

Select plugins in the initializer's declared argument order. Required dependencies must be installed in the same configuration; acquiring a missing capability later does not install it.

| Configuration field | Capability or service | Required dependencies |
| --- | --- | --- |
| `metadata: true` | Read and edit document Info dictionaries | None |
| `search: SearchConfig()` | Session search and independent all-hit queries | None |
| `render: RenderConfig()` | Whole-page, thumbnail, and tile rendering | None |
| `interaction: true` | Pointer routing and tool registration | None |
| `feedback: FeedbackConfig(...)` | Application-supplied feedback provider | A provider implementation |
| `stage: StageConfig()` | Viewport layout, camera, zoom, and navigation | None; pointer-driven stage scrolling additionally uses interaction |
| `selection: true` | Text selection and extraction | Interaction |
| `link: true` | Link hit areas and activation | Interaction |
| `annotation: AnnotationConfig()` | Annotation editing and comments | Interaction |
| `form: FormConfig()` | AcroForm filling and field design | Interaction |
| `redaction: RedactionConfig()` | Mark and apply redactions | Annotation and interaction |
| `measurement: MeasurementConfig()` | Calibration and measurement readouts | Annotation and interaction |
| `stamp: StampConfig()` | Workspace stamp library management | Annotation and interaction when placing stamps |
| `pageEdit: true` | Page rotation, movement, deletion, insertion, and extraction | None |
| `viewManager: true` | Workspace pane and tab organization | None |
| `shell: true` | Named surface and menu state | None |
| `i18n: I18nConfig(...)` | Locale packs and translation | Packs or a loader as needed |
| `commands: true` | Application command registration | None |
| `actions: ActionsConfig(...)` | PDF action dispatch and host ports | Supply ports for actions your app supports |
| `signature: SignatureConfig()` | Signature fields, analysis, validation, and signing | Form and interaction; signing also needs a signer |

Selection enables annotation markup from selected text. Redaction marking from search results additionally needs search; marking from the current selection needs selection. Those optional workflows are distinct from the base plugin dependencies.

For example, an annotation editor can start with this workspace:

```swift
import EmbedPDF

func makeAnnotationWorkspace() throws -> PDFViewer {
  try PDFViewer(ViewerConfig(
    render: RenderConfig(),
    interaction: true,
    stage: StageConfig(),
    selection: true,
    annotation: AnnotationConfig()
  ))
}
```

## Separate installation from permission

`scope: nil` requests the wildcard scope. Set explicit scope strings to restrict document operations. For a read-only renderer, for example:

```swift
import EmbedPDF

func makeRestrictedWorkspace() throws -> PDFViewer {
  try PDFViewer(ViewerConfig(
    scope: ["doc.open", "doc.render"],
    render: RenderConfig()
  ))
}
```

This viewer can open and render documents, but saving requires `doc.download`. Text search needs `doc.text.search`; full-text search output additionally needs `doc.text.copy`. Check the relevant capability's permission methods before enabling controls and handle a refused operation even after a successful check.

Embedded PDF permission bits are opt-in: `pdf.permissions` expands into the operations the file permits. A wildcard or an explicit operation grant bypasses those bits for that operation. To derive document operations from the file's permissions while explicitly permitting saving, use `scope: ["pdf.permissions", "doc.download"]`. Do not combine that scope with `"*"` when your intent is to honor embedded restrictions. Signature protection is applied independently and can still refuse an otherwise granted mutation.

Hiding a button does not enforce permission. Omitting a plugin makes its acquisition fail with `unsupported`; an installed operation can still fail with `permission-denied`.

## Verify the configuration

Start the workspace, open a ready document, and acquire each capability you intend to use. A missing dependency can fail viewer construction. If acquisition reports `not-ready`, inspect the document status before changing the configuration.

Next, follow <doc:EditingAndSaving> for mutation and persistence, or <doc:ExtendingTheViewer> for app-provided behavior.
