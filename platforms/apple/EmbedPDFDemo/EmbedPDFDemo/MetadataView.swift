import EmbedPDF
import SwiftUI

struct MetadataView: View {
  let metadata: DocumentMetadata
  let custom: CustomMetadata
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    NavigationStack {
      Form {
        Section("Document") {
          row("Title", metadata.title)
          row("Author", metadata.author)
          row("Subject", metadata.subject)
          row("Keywords", metadata.keywords)
        }
        Section("Producer") {
          row("Creator", metadata.creator)
          row("Producer", metadata.producer)
          row("Created", metadata.createdAt)
          row("Modified", metadata.modifiedAt)
        }
        if !custom.isEmpty {
          Section("Custom") {
            ForEach(custom.keys.sorted(), id: \.self) { key in
              row(key, custom[key])
            }
          }
        }
      }
      .navigationTitle("Info")
      .toolbar {
        Button("Done") { dismiss() }
      }
    }
  }

  private func row(_ label: String, _ value: String?) -> some View {
    LabeledContent(label, value: value ?? "–")
  }
}
