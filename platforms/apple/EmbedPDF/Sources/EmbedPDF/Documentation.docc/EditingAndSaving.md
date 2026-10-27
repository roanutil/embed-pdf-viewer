# Edit and save a document

Apply changes through a capability, then serialize the document and write the returned bytes.

## Before you begin

Enable the relevant plugins before starting the viewer. Metadata editing needs `metadata: true`. Annotation editing needs `interaction: true` and `AnnotationConfig()`. Form filling needs interaction and `FormConfig()`. See <doc:ConfiguringPlugins> for the full dependency table.

Your access scope must allow the mutation and `doc.download` for saving. To derive editing authority from embedded PDF permission bits, configure `pdf.permissions` instead of the default wildcard; explicit operation grants bypass those bits for the granted operation. Signature protection can refuse an operation regardless of its scope grant. Installing a capability or showing a control does not change these rules.

## Update document metadata

The following helper updates the title of an existing document and returns the complete saved PDF. The caller supplies a started viewer with metadata installed:

```swift
import EmbedPDF
import Foundation

func saveWithTitle(
  _ title: String, document: DocumentInfo, viewer: PDFViewer
) async throws -> Data {
  let metadata = try await viewer.metadata(for: document)
  _ = try await metadata.update(MetadataPatch(title: title))
  return try await viewer.save(document)
}
```

`canEdit()` can help decide whether to enable a metadata editor. Still handle failure from `update`, because state or policy can change. The metadata update returns after the confirmed change reaches its snapshot and events. Cancellation is not a promise to undo a write that already reached the engine.

For annotation and form workflows, use their capability methods and inspect returned batch failures when present. Refresh page layout after page edits. Refresh or invalidate the rendering state your custom UI depends on rather than assuming saved bytes automatically redraw an image.

## Choose a save mode

| Mode | Output | Constraint |
| --- | --- | --- |
| `.incremental` | A complete PDF with edits appended after the original bytes | Default; preserves earlier signed byte ranges. |
| `.rewrite` | A fresh serialization without the earlier revision history | Refused with `permission-denied` for signed documents. |

Preserving signed bytes does not prove certificate trust or that a subsequent modification satisfies certification policy. Use ``SignatureCapability`` to inspect and validate signatures when your workflow depends on those properties.

Saving returns `Data`; your app chooses the destination and replacement policy. This complete helper saves a live document and writes to a caller-provided writable URL:

```swift
import EmbedPDF
import Foundation

func writeDocument(
  _ document: DocumentInfo, viewer: PDFViewer, to destination: URL
) async throws {
  let bytes = try await viewer.save(document, mode: .incremental)
  try bytes.write(to: destination, options: .atomic)
}
```

This helper can throw either an SDK error or a Foundation file-writing error. Use file coordination or document-provider APIs when your destination requires them, and keep security-scoped access active where applicable. For large files, arrange file writing off the main actor. A successful `save` alone does not mean a file was persisted.

## Treat content removal and signing as separate workflows

Redaction marking is a preview, not content removal. Applying redactions rewrites document content; inspect the result, save it, and verify the output. An incremental PDF can retain prior bytes and revisions, so it is unsuitable when the required result must omit earlier sensitive content. For an unsigned document, use a rewrite save for that output and verify it independently; do not promise that marking or painting a rectangle removes data from a file.

Signing requires the signature plugin, its form and interaction dependencies, a signer, and appropriate permission. Validation additionally depends on the trust configuration. ``KeychainSigner`` adapts a supplied Security identity to a detached CMS signer; its initializer does not by itself establish where that private key resides. Read the signature method contracts before presenting a trust verdict or a completed signing state.

## Verify persisted output

Open the returned bytes or saved destination as a new document and confirm the intended content or metadata. For batches, also inspect per-item failures. For signed or redacted output, verify the properties your application promises rather than checking only that a save call returned.

Next, follow <doc:ErrorsAndObservation> to refresh UI after confirmed mutations.

## Related APIs

- ``MetadataCapability``
- ``AnnotationCapability``
- ``FormCapability``
- ``PDFViewer/save(_:mode:)``
- ``SignatureCapability``
