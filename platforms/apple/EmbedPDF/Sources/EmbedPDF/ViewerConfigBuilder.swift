public import EmbedPDFBindings

/// A chained way to build a `ViewerConfig`. Every method returns a changed
/// copy, so a builder is a plain value:
///
///     let viewer = try PDFViewer(
///       ViewerConfigBuilder().interaction().annotation(settingsJSON: "").search(SearchConfig()))
///
/// An unset field is the engine's default for an absent field: a plugin is
/// left out (`false` or `nil`, `ViewerConfig`'s `#[uniffi(default = ...)]` in
/// `crates/epdf-viewer-host/src/api.rs`, read by `Session::build` in
/// `session.rs`, where `if config.metadata` and `if let Some(search) = ...`
/// install a plugin only when it is set) and `scope` is `nil`, which the
/// engine takes as `["*"]` (`api.rs`, `Session::build`'s `NativeEngine::new`).
/// A builder with no calls builds a config equal to `ViewerConfig()`.
///
/// The builder exists so that a plugin field added to `ViewerConfig` is one
/// new method here and no change for a caller. `ViewerConfig(...)` keeps
/// working as it is. One method per field is checked by
/// `ViewerConfigBuilderTests`, which fails when the two drift apart.
public struct ViewerConfigBuilder: Sendable {
  private var config: ViewerConfig

  /// Starts from `ViewerConfig()`: every plugin left out.
  public init() { config = ViewerConfig() }

  /// Starts from `config`, to change a few of its fields.
  public init(_ config: ViewerConfig) { self.config = config }

  /// The `ViewerConfig` the calls so far describe.
  public func build() -> ViewerConfig { config }

  private func changing(_ change: (inout ViewerConfig) -> Void) -> ViewerConfigBuilder {
    var copy = self
    change(&copy.config)
    return copy
  }

  /// The capability scope every document opens with. `nil` is `["*"]`.
  public func scope(_ scope: [String]?) -> ViewerConfigBuilder { changing { $0.scope = scope } }

  /// Installs the metadata plugin (`enabled`), or leaves it out.
  public func metadata(_ enabled: Bool = true) -> ViewerConfigBuilder { changing { $0.metadata = enabled } }

  public func search(_ config: SearchConfig) -> ViewerConfigBuilder { changing { $0.search = config } }

  public func render(_ config: RenderConfig) -> ViewerConfigBuilder { changing { $0.render = config } }

  /// The pointer hub that selection, link, annotation, form and measurement need.
  public func interaction(_ enabled: Bool = true) -> ViewerConfigBuilder { changing { $0.interaction = enabled } }

  public func feedback(_ config: FeedbackConfig) -> ViewerConfigBuilder { changing { $0.feedback = config } }

  public func stage(_ config: StageConfig) -> ViewerConfigBuilder { changing { $0.stage = config } }

  /// Needs `interaction`.
  public func selection(_ enabled: Bool = true) -> ViewerConfigBuilder { changing { $0.selection = enabled } }

  /// Needs `interaction`.
  public func link(_ enabled: Bool = true) -> ViewerConfigBuilder { changing { $0.link = enabled } }

  /// Installs the annotation plugin. `settingsJSON` is registered over the
  /// defaults; `""` is the defaults. Needs `interaction`.
  public func annotation(settingsJSON: String = "") -> ViewerConfigBuilder {
    changing { $0.annotation = settingsJSON }
  }

  public func history(_ enabled: Bool = true) -> ViewerConfigBuilder { changing { $0.history = enabled } }

  public func outline(_ enabled: Bool = true) -> ViewerConfigBuilder { changing { $0.outline = enabled } }

  public func attachment(_ enabled: Bool = true) -> ViewerConfigBuilder { changing { $0.attachment = enabled } }

  /// Needs `interaction`.
  public func form(_ config: FormConfig) -> ViewerConfigBuilder { changing { $0.form = config } }

  /// Needs `annotation`.
  public func redaction(_ config: RedactionConfig) -> ViewerConfigBuilder { changing { $0.redaction = config } }

  /// Needs `interaction` and `annotation`.
  public func measurement(_ config: MeasurementConfig) -> ViewerConfigBuilder { changing { $0.measurement = config } }

  public func stamp(_ config: StampConfig) -> ViewerConfigBuilder { changing { $0.stamp = config } }

  public func pageEdit(_ enabled: Bool = true) -> ViewerConfigBuilder { changing { $0.pageEdit = enabled } }

  public func viewManager(_ enabled: Bool = true) -> ViewerConfigBuilder { changing { $0.viewManager = enabled } }

  public func shell(_ enabled: Bool = true) -> ViewerConfigBuilder { changing { $0.shell = enabled } }

  public func i18n(_ config: I18nConfig) -> ViewerConfigBuilder { changing { $0.i18n = config } }

  public func commands(_ enabled: Bool = true) -> ViewerConfigBuilder { changing { $0.commands = enabled } }

  public func actions(_ config: ActionsConfig) -> ViewerConfigBuilder { changing { $0.actions = config } }

  /// Needs `form`.
  public func signature(_ config: SignatureConfig) -> ViewerConfigBuilder { changing { $0.signature = config } }
}

extension PDFViewer {
  /// `PDFViewer(_:documentFetcher:)` for a config described by a builder.
  public convenience init(_ builder: ViewerConfigBuilder, documentFetcher: any DocumentFetcher = URLSessionDocumentFetcher()) throws(PDFError) {
    try self.init(builder.build(), documentFetcher: documentFetcher)
  }
}
