import SwiftUI

struct ContentView: View {
    @State private var model = DocumentModel()

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if let failure = model.failure {
                    Text(failure)
                        .font(.system(.body, design: .monospaced))
                        .foregroundStyle(.red)
                }

                LabeledContent("Pages", value: "\(model.pageCount)")
                if let size = model.pageSize {
                    LabeledContent("Page 4", value: String(format: "%.1f x %.1f pt", size.width, size.height))
                }
                LabeledContent("Hits for \"the\"", value: "\(model.searchHitCount)")

                if let image = model.image {
                    Image(decorative: image, scale: 2.0)
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                        .border(.separator)
                }

                Text(model.textPreview)
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
            }
            .padding()
        }
        .task { model.load() }
        .onDisappear { model.close() }
    }
}
