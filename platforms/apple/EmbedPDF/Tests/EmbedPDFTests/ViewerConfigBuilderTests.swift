import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  /// `ViewerConfigBuilder` has one method per `ViewerConfig` field, and an
  /// unset field is the generated default. `everyFieldHasARow` fails when
  /// `ViewerConfig` gains a field without a row here, and a row only passes
  /// when its method sets that field and no other.
  @Suite struct ViewerConfigBuilderTests {
    struct Row: Sendable, CustomTestStringConvertible {
      /// The `ViewerConfig` field the method sets.
      let field: String
      /// Calls the builder method for `field` with a value that is not the default.
      let set: @Sendable (ViewerConfigBuilder) -> ViewerConfigBuilder

      var testDescription: String { field }

      init(_ field: String, set: @escaping @Sendable (ViewerConfigBuilder) -> ViewerConfigBuilder) {
        self.field = field
        self.set = set
      }
    }

    static let rows: [Row] = [
      Row("scope") { $0.scope(["doc.open"]) },
      Row("metadata") { $0.metadata() },
      Row("search") { $0.search(SearchConfig()) },
      Row("render") { $0.render(RenderConfig()) },
      Row("interaction") { $0.interaction() },
      Row("feedback") { $0.feedback(FeedbackConfig(provider: RecordingFeedback())) },
      Row("stage") { $0.stage(StageConfig()) },
      Row("selection") { $0.selection() },
      Row("link") { $0.link() },
      Row("annotation") { $0.annotation(settingsJSON: "") },
      Row("history") { $0.history() },
      Row("outline") { $0.outline() },
      Row("attachment") { $0.attachment() },
      Row("form") { $0.form(FormConfig()) },
      Row("redaction") { $0.redaction(RedactionConfig()) },
      Row("measurement") { $0.measurement(MeasurementConfig()) },
      Row("stamp") { $0.stamp(StampConfig()) },
      Row("pageEdit") { $0.pageEdit() },
      Row("viewManager") { $0.viewManager() },
      Row("shell") { $0.shell() },
      Row("i18n") { $0.i18n(I18nConfig()) },
      Row("commands") { $0.commands() },
      Row("actions") { $0.actions(ActionsConfig(openSequence: .off)) },
      Row("signature") { $0.signature(SignatureConfig()) },
    ]

    /// Each field's value as text, in declaration order, so two configs
    /// compare field by field (`ViewerConfig` is not `Equatable`).
    static func fields(_ config: ViewerConfig) -> [(label: String, value: String)] {
      Mirror(reflecting: config).children.map { child in
        (child.label ?? "", String(customDumping: child.value))
      }
    }

    @Test func everyFieldHasARow() {
      let fields = Set(Self.fields(ViewerConfig()).map(\.label))
      let rows = Self.rows.map(\.field)
      expectNoDifference(Set(rows), fields)
      expectNoDifference(rows.count, Set(rows).count)
    }

    @Test func anUnconfiguredBuilderIsTheGeneratedDefault() {
      let built = Self.fields(ViewerConfigBuilder().build())
      let generated = Self.fields(ViewerConfig())
      expectNoDifference(built.map(\.label), generated.map(\.label))
      expectNoDifference(built.map(\.value), generated.map(\.value))
    }

    @Test(arguments: rows)
    func aMethodSetsItsFieldAndNothingElse(_ row: Row) {
      let defaults = Self.fields(ViewerConfig())
      let built = Self.fields(row.set(ViewerConfigBuilder()).build())
      expectNoDifference(built.map(\.label), defaults.map(\.label))
      for (built, generated) in zip(built, defaults) {
        if built.label == row.field {
          #expect(built.value != generated.value, "\(row.field) was not set")
        } else {
          #expect(built.value == generated.value, "\(row.field) changed \(built.label)")
        }
      }
    }

    @Test func aBuilderIsAValue() {
      let base = ViewerConfigBuilder().interaction()
      let changed = base.selection()
      expectNoDifference(Self.fields(base.build()).filter { $0.label == "selection" }.map(\.value), ["false"])
      expectNoDifference(Self.fields(changed.build()).filter { $0.label == "selection" }.map(\.value), ["true"])
    }

    @Test func aBuilderStartsFromAnExistingConfig() {
      let config = ViewerConfigBuilder(ViewerConfig(history: true)).outline().build()
      #expect(config.history && config.outline && !config.metadata)
    }

    /// The accessors are the ones `DisabledPluginTests` checks; here the
    /// config comes from the builder and the engine decides.
    @Test(arguments: DisabledPluginTests.accessors)
    func aPluginTheBuilderLeavesOutIsUnsupported(_ accessor: DisabledPluginTests.Accessor) async throws {
      var prerequisites = ViewerConfig()
      accessor.prerequisites(&prerequisites)
      let viewer = try PDFViewer(ViewerConfigBuilder(prerequisites))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      await #expect {
        try await accessor.call(viewer, document)
      } throws: { error in
        (error as? PDFError)?.code == .unsupported
      }
      try await viewer.destroy()
    }

    @Test(arguments: DisabledPluginTests.accessors)
    func aPluginTheBuilderInstallsHasAnAccessor(_ accessor: DisabledPluginTests.Accessor) async throws {
      let row = try #require(Self.rows.first { $0.field == accessor.field })
      var prerequisites = ViewerConfig()
      accessor.prerequisites(&prerequisites)
      let viewer = try PDFViewer(row.set(ViewerConfigBuilder(prerequisites)))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      try await accessor.call(viewer, document)
      try await viewer.destroy()
    }
  }
}
