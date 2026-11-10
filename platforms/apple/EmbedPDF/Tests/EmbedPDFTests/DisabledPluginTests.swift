import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  /// A plugin left out of `ViewerConfig` is refused at its accessor, before
  /// any call on a capability: the host asks the kernel whether the plugin
  /// is installed (`kernel.provides`) ahead of any document lookup and answers
  /// `unsupported`. A capability the accessor returns has its own plugin; a
  /// few of its calls still refuse when a companion plugin is left out (see
  /// the Plugins section of `platforms/apple/README.md`).
  ///
  /// One row per accessor. `everyPluginInViewerConfigHasARow` fails when
  /// `ViewerConfig` gains a plugin field without a row here.
  @Suite struct DisabledPluginTests {
    struct Accessor: Sendable, CustomTestStringConvertible {
      /// The `ViewerConfig` field that installs the plugin.
      let field: String
      /// Installs the plugins this one needs to build, leaving it out.
      let prerequisites: @Sendable (inout ViewerConfig) -> Void
      /// Installs the plugin itself.
      let install: @Sendable (inout ViewerConfig) -> Void
      /// Calls the accessor and discards the capability.
      let call: @Sendable (PDFViewer, DocumentInfo) async throws -> Void

      var testDescription: String { field }

      init(
        _ field: String,
        prerequisites: @escaping @Sendable (inout ViewerConfig) -> Void = { _ in },
        install: @escaping @Sendable (inout ViewerConfig) -> Void,
        call: @escaping @Sendable (PDFViewer, DocumentInfo) async throws -> Void
      ) {
        self.field = field
        self.prerequisites = prerequisites
        self.install = install
        self.call = call
      }

      func viewer(installed: Bool) async throws -> (PDFViewer, DocumentInfo) {
        var config = ViewerConfig()
        prerequisites(&config)
        if installed { install(&config) }
        let viewer = try PDFViewer(config)
        try await viewer.start()
        return (viewer, try await viewer.open(Fixture.data("report")))
      }
    }

    static let interaction: @Sendable (inout ViewerConfig) -> Void = { $0.interaction = true }
    static let annotation: @Sendable (inout ViewerConfig) -> Void = {
      $0.interaction = true
      $0.annotation = ""
    }

    static let accessors: [Accessor] = [
      Accessor("metadata", install: { $0.metadata = true }) { _ = try await $0.metadata(for: $1) },
      Accessor("search", install: { $0.search = SearchConfig() }) { _ = try await $0.search(in: $1) },
      Accessor("render", install: { $0.render = RenderConfig() }) { _ = try await $0.render(in: $1) },
      Accessor("interaction", install: { $0.interaction = true }) {
        _ = try await $0.interaction(for: $1)
      },
      Accessor("feedback", install: { $0.feedback = FeedbackConfig(provider: RecordingFeedback()) }) {
        viewer, _ in _ = try await viewer.feedback()
      },
      Accessor("stage", install: { $0.stage = StageConfig() }) { _ = try await $0.stage(for: $1) },
      Accessor("selection", prerequisites: interaction, install: { $0.selection = true }) {
        _ = try await $0.selection(for: $1)
      },
      Accessor("link", prerequisites: interaction, install: { $0.link = true }) {
        _ = try await $0.link(for: $1)
      },
      Accessor("annotation", prerequisites: interaction, install: { $0.annotation = "" }) {
        _ = try await $0.annotations(for: $1)
      },
      Accessor("history", install: { $0.history = true }) { _ = try await $0.history(for: $1) },
      Accessor("outline", install: { $0.outline = true }) { _ = try await $0.outline(for: $1) },
      Accessor("attachment", install: { $0.attachment = true }) {
        _ = try await $0.attachments(for: $1)
      },
      Accessor("form", prerequisites: interaction, install: { $0.form = FormConfig() }) {
        _ = try await $0.form(for: $1)
      },
      Accessor("redaction", prerequisites: annotation, install: { $0.redaction = RedactionConfig() }) {
        _ = try await $0.redaction(for: $1)
      },
      Accessor("measurement", prerequisites: annotation, install: { $0.measurement = MeasurementConfig() }) {
        _ = try await $0.measurement(for: $1)
      },
      Accessor("stamp", install: { $0.stamp = StampConfig() }) { viewer, _ in
        _ = try await viewer.stamps()
      },
      Accessor("pageEdit", install: { $0.pageEdit = true }) { _ = try await $0.pageEdit(for: $1) },
      Accessor("viewManager", install: { $0.viewManager = true }) { viewer, _ in
        _ = try await viewer.viewManager()
      },
      Accessor("shell", install: { $0.shell = true }) { _ = try await $0.shell(for: $1) },
      Accessor("i18n", install: { $0.i18n = I18nConfig() }) { viewer, _ in
        _ = try await viewer.i18n()
      },
      Accessor("commands", install: { $0.commands = true }) { viewer, _ in
        _ = try await viewer.commands()
      },
      Accessor("actions", install: { $0.actions = ActionsConfig(openSequence: .off) }) {
        _ = try await $0.actions(for: $1)
      },
      Accessor(
        "signature",
        prerequisites: {
          $0.interaction = true
          $0.form = FormConfig()
        },
        install: { $0.signature = SignatureConfig() }
      ) { _ = try await $0.signatures(for: $1) },
    ]

    @Test func everyPluginInViewerConfigHasARow() {
      let fields = Set(Mirror(reflecting: ViewerConfig()).children.compactMap { $0.label })
        .subtracting(["scope"])
      let rows = Self.accessors.map(\.field)
      expectNoDifference(Set(rows), fields)
      expectNoDifference(rows.count, Set(rows).count)
    }

    @Test(arguments: accessors)
    func aPluginLeftOutIsUnsupportedAtTheAccessor(_ accessor: Accessor) async throws {
      let (viewer, document) = try await accessor.viewer(installed: false)
      await #expect {
        try await accessor.call(viewer, document)
      } throws: { error in
        (error as? PDFError)?.code == .unsupported
      }
      try await viewer.destroy()
    }

    @Test(arguments: accessors)
    func anInstalledPluginsAccessorSucceeds(_ accessor: Accessor) async throws {
      let (viewer, document) = try await accessor.viewer(installed: true)
      try await accessor.call(viewer, document)
      try await viewer.destroy()
    }

    /// The refusal does not depend on the document: a document the viewer
    /// has closed is still `unsupported`, not `not-found`, when the plugin is
    /// left out.
    @Test(arguments: accessors)
    func aPluginLeftOutIsUnsupportedForAClosedDocument(_ accessor: Accessor) async throws {
      var config = ViewerConfig()
      accessor.prerequisites(&config)
      let viewer = try PDFViewer(config)
      try await viewer.start()
      let opened = try await viewer.open(Fixture.data("report"))
      try await viewer.close(opened)
      await #expect {
        try await accessor.call(viewer, opened)
      } throws: { error in
        (error as? PDFError)?.code == .unsupported
      }
      try await viewer.destroy()
    }

    /// A document still loading is `unsupported`, not `not-ready`, when the
    /// plugin is left out.
    @Test(arguments: accessors)
    func aPluginLeftOutIsUnsupportedForALoadingDocument(_ accessor: Accessor) async throws {
      var config = ViewerConfig()
      accessor.prerequisites(&config)
      let viewer = try PDFViewer(config, documentFetcher: HeldURLProtocol.fetcher())
      try await viewer.start()
      let opening = Task { try await viewer.open(URL(string: "https://embedpdf.test/held.pdf")!) }
      var loading: DocumentInfo?
      for _ in 0..<500 {
        loading = try await viewer.documents().first
        if loading != nil { break }
        try await Task.sleep(for: .milliseconds(10))
      }
      let document = try #require(loading)
      #expect(document.status == .loading)
      await #expect {
        try await accessor.call(viewer, document)
      } throws: { error in
        (error as? PDFError)?.code == .unsupported
      }
      opening.cancel()
      _ = await opening.result
      try await viewer.destroy()
    }
  }
}

/// Answers no request, so an open through it stays pending and its tab
/// stays `.loading` until the open is cancelled.
private final class HeldURLProtocol: URLProtocol, @unchecked Sendable {
  override class func canInit(with request: URLRequest) -> Bool { true }
  override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
  override func startLoading() {}
  override func stopLoading() {}

  static func fetcher() -> URLSessionDocumentFetcher {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [HeldURLProtocol.self]
    return URLSessionDocumentFetcher(session: URLSession(configuration: configuration))
  }
}
