import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  /// `PageNavigator`, `PageZoom` and `ToolSwitcher` add names, not
  /// behaviour. Every member forwards to the capability member of the same
  /// name: `forwards` proves at compile time that the two exist with the
  /// same signature and errors, the source audit proves each façade member
  /// is listed there and is a one-line forward, and the equivalence tests
  /// run each member both ways on fresh documents and compare what the
  /// engine reports.
  @Suite struct TaskEntryPointTests {
    // ── forwarding table ──

    /// Compiles only while the façade member and the capability member
    /// exist with the same parameters, result and typed error.
    static func forward<each Argument, Result>(
      _ name: String,
      _ facade: (PageNavigator) -> (repeat each Argument) async throws(PDFError) -> Result,
      _ capability: (StageCapability) -> (repeat each Argument) async throws(PDFError) -> Result
    ) -> String { name }

    static func forward<each Argument, Result>(
      _ name: String,
      _ facade: (PageZoom) -> (repeat each Argument) async throws(PDFError) -> Result,
      _ capability: (StageCapability) -> (repeat each Argument) async throws(PDFError) -> Result
    ) -> String { name }

    static func forward<each Argument, Result>(
      _ name: String,
      _ facade: (ToolSwitcher) -> (repeat each Argument) async throws(PDFError) -> Result,
      _ capability: (InteractionCapability) -> (repeat each Argument) async throws(PDFError) -> Result
    ) -> String { name }

    static let forwards: [String: [String]] = [
      "PageNavigator": [
        forward("pageCount", PageNavigator.pageCount, StageCapability.pageCount),
        forward("currentPageIndex", PageNavigator.currentPageIndex, StageCapability.currentPageIndex),
        forward("currentPage", PageNavigator.currentPage, StageCapability.currentPage),
        forward("canGoNext", PageNavigator.canGoNext, StageCapability.canGoNext),
        forward("canGoPrevious", PageNavigator.canGoPrevious, StageCapability.canGoPrevious),
        forward("goToPageIndex", PageNavigator.goToPageIndex, StageCapability.goToPageIndex),
        forward("goToPage", PageNavigator.goToPage, StageCapability.goToPage),
        forward("goToFirstPage", PageNavigator.goToFirstPage, StageCapability.goToFirstPage),
        forward("goToLastPage", PageNavigator.goToLastPage, StageCapability.goToLastPage),
        forward("nextPage", PageNavigator.nextPage, StageCapability.nextPage),
        forward("previousPage", PageNavigator.previousPage, StageCapability.previousPage),
      ],
      "PageZoom": [
        forward("zoomLevel", PageZoom.zoomLevel, StageCapability.zoomLevel),
        forward("zoomMode", PageZoom.zoomMode, StageCapability.zoomMode),
        forward("canZoomIn", PageZoom.canZoomIn, StageCapability.canZoomIn),
        forward("canZoomOut", PageZoom.canZoomOut, StageCapability.canZoomOut),
        forward("zoomIn", PageZoom.zoomIn, StageCapability.zoomIn),
        forward("zoomOut", PageZoom.zoomOut, StageCapability.zoomOut),
        forward("zoomTo", PageZoom.zoomTo, StageCapability.zoomTo),
        forward("fitWidth", PageZoom.fitWidth, StageCapability.fitWidth),
        forward("fitPage", PageZoom.fitPage, StageCapability.fitPage),
      ],
      "ToolSwitcher": [
        forward("tools", ToolSwitcher.tools, InteractionCapability.tools),
        forward("activeToolID", ToolSwitcher.activeToolID, InteractionCapability.activeToolID),
        forward("defaultToolID", ToolSwitcher.defaultToolID, InteractionCapability.defaultToolID),
        forward("hasTool", ToolSwitcher.hasTool, InteractionCapability.hasTool),
        forward("activateTool", ToolSwitcher.activateTool, InteractionCapability.activateTool),
        forward("activateDefaultTool", ToolSwitcher.activateDefaultTool, InteractionCapability.activateDefaultTool),
        forward("pushTool", ToolSwitcher.pushTool, InteractionCapability.pushTool),
        forward("popTool", ToolSwitcher.popTool, InteractionCapability.popTool),
      ],
    ]

    /// The property each façade forwards through, as the source spells it.
    static let capabilityProperty = ["PageNavigator": "stage", "PageZoom": "stage", "ToolSwitcher": "interaction"]

    /// Compiles only while the viewer accessor exists with this spelling and type.
    static func accessor<Capability>(
      _ name: String, _ accessor: (PDFViewer) -> (DocumentInfo) async throws(PDFError) -> Capability,
      returning: Capability.Type
    ) -> String { name }

    /// The capability accessors keep their spelling and type beside the new ones.
    static let accessors = [
      accessor("stage(for:)", PDFViewer.stage(for:), returning: StageCapability.self),
      accessor("interaction(for:)", PDFViewer.interaction(for:), returning: InteractionCapability.self),
      accessor("pageNavigator(for:)", PDFViewer.pageNavigator(for:), returning: PageNavigator.self),
      accessor("pageZoom(for:)", PDFViewer.pageZoom(for:), returning: PageZoom.self),
      accessor("toolSwitcher(for:)", PDFViewer.toolSwitcher(for:), returning: ToolSwitcher.self),
    ]

    // ── source audit ──

    static func source() throws -> String {
      let package = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
      return try String(
        contentsOf: package.appending(path: "Sources/EmbedPDF/Plugins/TaskEntryPoints.swift"), encoding: .utf8)
    }

    /// The `public func` lines inside `public struct <type>`.
    static func members(of type: String, in source: String) throws -> [String] {
      let start = try #require(source.range(of: "public struct \(type): Sendable {"))
      let body = source[start.upperBound...]
      let end = try #require(body.range(of: "\n}\n"))
      return body[..<end.lowerBound].components(separatedBy: "\n")
        .map { $0.trimmingCharacters(in: .whitespaces) }
        .filter { $0.hasPrefix("public func ") }
    }

    @Test(arguments: ["PageNavigator", "PageZoom", "ToolSwitcher"])
    func everyFacadeMemberIsAOneLineForwardListedInTheTable(type: String) throws {
      let lines = try Self.members(of: type, in: try Self.source())
      let property = try #require(Self.capabilityProperty[type])
      var names: [String] = []
      for line in lines {
        let name = String(line.dropFirst("public func ".count).prefix { $0 != "(" })
        names.append(name)
        #expect(line.hasSuffix("}"), "\(type).\(name) spans more than one line")
        #expect(line.contains("{ try await \(property).\(name)("), "\(type).\(name) doesn't forward to \(property).\(name)")
      }
      expectNoDifference(names.sorted(), try #require(Self.forwards[type]).sorted())
    }

    @Test(arguments: ["PageNavigator", "PageZoom", "ToolSwitcher"])
    func everyFacadeMemberHasAnEquivalenceTest(type: String) throws {
      let tested = Set(
        Self.navigatorSteps.map { "PageNavigator.\($0.name)" } + Self.zoomSteps.map { "PageZoom.\($0.name)" }
          + Self.toolSteps.map { "ToolSwitcher.\($0.name)" })
      for name in try #require(Self.forwards[type]) {
        #expect(tested.contains("\(type).\(name)"), "\(type).\(name) has no equivalence step")
      }
    }

    // ── equivalence on the stage ──

    static let instant = StageGoToOptions(behavior: .instant)

    /// A fresh document and the capability or façade a step calls.
    struct Opened<Target: Sendable>: Sendable {
      let target: Target
      let viewer: PDFViewer
      let document: DocumentInfo

      func page(_ index: Int) async throws -> PageRef { try await viewer.pages(of: document)[index].page }
    }

    /// One façade member and the capability call it forwards to. Each side
    /// runs on its own fresh document. A read returns what it read, so the
    /// two results are compared as well as the state the call leaves.
    struct Step<Capability: Sendable, Facade: Sendable>: Sendable, CustomTestStringConvertible {
      let name: String
      let viaCapability: @Sendable (Opened<Capability>) async throws -> String
      let viaFacade: @Sendable (Opened<Facade>) async throws -> String
      var testDescription: String { name }
    }

    static let navigatorSteps: [Step<StageCapability, PageNavigator>] = [
      Step(name: "pageCount") { "\(try await $0.target.pageCount())" } viaFacade: { "\(try await $0.target.pageCount())" },
      Step(name: "currentPageIndex") { "\(try await $0.target.currentPageIndex())" } viaFacade: {
        "\(try await $0.target.currentPageIndex())"
      },
      Step(name: "currentPage") { "\(String(describing: try await $0.target.currentPage()))" } viaFacade: {
        "\(String(describing: try await $0.target.currentPage()))"
      },
      Step(name: "canGoNext") { "\(try await $0.target.canGoNext())" } viaFacade: { "\(try await $0.target.canGoNext())" },
      Step(name: "canGoPrevious") { "\(try await $0.target.canGoPrevious())" } viaFacade: {
        "\(try await $0.target.canGoPrevious())"
      },
      Step(name: "goToPageIndex") { try await $0.target.goToPageIndex(5, options: instant); return "" } viaFacade: {
        try await $0.target.goToPageIndex(5, options: instant); return ""
      },
      Step(name: "goToPage") { try await $0.target.goToPage(try await $0.page(6), options: instant); return "" }
        viaFacade: { try await $0.target.goToPage(try await $0.page(6), options: instant); return "" },
      Step(name: "goToFirstPage") { try await $0.target.goToFirstPage(options: instant); return "" } viaFacade: {
        try await $0.target.goToFirstPage(options: instant); return ""
      },
      Step(name: "goToLastPage") { try await $0.target.goToLastPage(options: instant); return "" } viaFacade: {
        try await $0.target.goToLastPage(options: instant); return ""
      },
      Step(name: "nextPage") { try await $0.target.nextPage(options: instant); return "" } viaFacade: {
        try await $0.target.nextPage(options: instant); return ""
      },
      Step(name: "previousPage") { try await $0.target.previousPage(options: instant); return "" } viaFacade: {
        try await $0.target.previousPage(options: instant); return ""
      },
    ]

    static let zoomSteps: [Step<StageCapability, PageZoom>] = [
      Step(name: "zoomLevel") { "\(try await $0.target.zoomLevel())" } viaFacade: { "\(try await $0.target.zoomLevel())" },
      Step(name: "zoomMode") { "\(try await $0.target.zoomMode())" } viaFacade: { "\(try await $0.target.zoomMode())" },
      Step(name: "canZoomIn") { "\(try await $0.target.canZoomIn())" } viaFacade: { "\(try await $0.target.canZoomIn())" },
      Step(name: "canZoomOut") { "\(try await $0.target.canZoomOut())" } viaFacade: {
        "\(try await $0.target.canZoomOut())"
      },
      Step(name: "zoomIn") { try await $0.target.zoomIn(); return "" } viaFacade: { try await $0.target.zoomIn(); return "" },
      Step(name: "zoomOut") { try await $0.target.zoomOut(); return "" } viaFacade: { try await $0.target.zoomOut(); return "" },
      Step(name: "zoomTo") { try await $0.target.zoomTo(.level(level: 2), around: Point(x: 100, y: 200)); return "" }
        viaFacade: { try await $0.target.zoomTo(.level(level: 2), around: Point(x: 100, y: 200)); return "" },
      Step(name: "fitWidth") { try await $0.target.fitWidth(); return "" } viaFacade: { try await $0.target.fitWidth(); return "" },
      Step(name: "fitPage") { try await $0.target.fitPage(); return "" } viaFacade: { try await $0.target.fitPage(); return "" },
    ]

    struct StageState: Equatable {
      var pageIndex: Int
      var page: PageRef?
      var zoomLevel: Double
      var zoomMode: StageZoomMode
      var camera: Camera
    }

    /// Sized, and on page 3 so every navigation step has somewhere to go.
    func openedStage() async throws -> (PDFViewer, DocumentInfo, StageCapability) {
      let viewer = try PDFViewer(ViewerConfig(stage: StageConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let stage = try await viewer.stage(for: document)
      try await stage.setViewportSize(Size(width: 390, height: 844))
      try await stage.goToPageIndex(3, options: Self.instant)
      return (viewer, document, stage)
    }

    /// What the engine reports once any tween has finished.
    func observe(_ stage: StageCapability) async throws -> StageState {
      var polls = 0
      while try await stage.isMoving(), polls < 500 {
        polls += 1
        try await Task.sleep(for: .milliseconds(10))
      }
      #expect(try await !stage.isMoving())
      return StageState(
        pageIndex: try await stage.currentPageIndex(), page: try await stage.currentPage(),
        zoomLevel: try await stage.zoomLevel(), zoomMode: try await stage.zoomMode(), camera: try await stage.camera())
    }

    func compareOnTheStage<Facade: Sendable>(
      _ step: Step<StageCapability, Facade>,
      facade: @Sendable (PDFViewer, DocumentInfo) async throws -> Facade
    ) async throws {
      let (viewer, document, stage) = try await openedStage()
      let before = try await observe(stage)
      let capabilityRead = try await step.viaCapability(Opened(target: stage, viewer: viewer, document: document))
      let capabilityState = try await observe(stage)

      let (facadeViewer, facadeDocument, facadeStage) = try await openedStage()
      let target = try await facade(facadeViewer, facadeDocument)
      let facadeRead = try await step.viaFacade(Opened(target: target, viewer: facadeViewer, document: facadeDocument))
      let facadeState = try await observe(facadeStage)

      expectNoDifference(facadeRead, capabilityRead)
      expectNoDifference(facadeState, capabilityState)
      if capabilityRead.isEmpty {
        #expect(capabilityState != before, "\(step.name) should change what the stage shows")
      }
    }

    @Test(arguments: navigatorSteps)
    func pageNavigatorMemberMatchesTheStage(step: Step<StageCapability, PageNavigator>) async throws {
      try await compareOnTheStage(step) { try await $0.pageNavigator(for: $1) }
    }

    @Test(arguments: zoomSteps)
    func pageZoomMemberMatchesTheStage(step: Step<StageCapability, PageZoom>) async throws {
      try await compareOnTheStage(step) { try await $0.pageZoom(for: $1) }
    }

    // ── equivalence on the pointer hub ──

    static let toolSteps: [Step<InteractionCapability, ToolSwitcher>] = [
      Step(name: "tools") { "\(try await $0.target.tools())" } viaFacade: { "\(try await $0.target.tools())" },
      Step(name: "activeToolID") { "\(try await $0.target.activeToolID())" } viaFacade: {
        "\(try await $0.target.activeToolID())"
      },
      Step(name: "defaultToolID") { "\(try await $0.target.defaultToolID())" } viaFacade: {
        "\(try await $0.target.defaultToolID())"
      },
      Step(name: "hasTool") { "\(try await $0.target.hasTool(.zoom))" } viaFacade: { "\(try await $0.target.hasTool(.zoom))" },
      Step(name: "activateTool") { try await $0.target.activateTool(.zoom); return "" } viaFacade: {
        try await $0.target.activateTool(.zoom); return ""
      },
      Step(name: "activateDefaultTool") { try await $0.target.activateDefaultTool(); return "" } viaFacade: {
        try await $0.target.activateDefaultTool(); return ""
      },
      Step(name: "pushTool") { try await $0.target.pushTool(.zoom); return "" } viaFacade: {
        try await $0.target.pushTool(.zoom); return ""
      },
      Step(name: "popTool") { try await $0.target.popTool(); return "" } viaFacade: { try await $0.target.popTool(); return "" },
    ]

    /// Starts on `pan` pushed over the default tool, so `activateDefaultTool`
    /// and `popTool` both have something to change.
    func openedInteraction() async throws -> (PDFViewer, DocumentInfo, InteractionCapability) {
      let viewer = try PDFViewer(ViewerConfig(interaction: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let interaction = try await viewer.interaction(for: document)
      try await interaction.pushTool(.pan)
      return (viewer, document, interaction)
    }

    func observe(_ interaction: InteractionCapability) async throws -> [String] {
      [try await interaction.activeToolID().rawValue, try await interaction.cursor()]
    }

    @Test(arguments: toolSteps)
    func toolSwitcherMemberMatchesTheHub(step: Step<InteractionCapability, ToolSwitcher>) async throws {
      let (viewer, document, interaction) = try await openedInteraction()
      let before = try await observe(interaction)
      let capabilityRead = try await step.viaCapability(Opened(target: interaction, viewer: viewer, document: document))
      let capabilityState = try await observe(interaction)

      let (facadeViewer, facadeDocument, facadeInteraction) = try await openedInteraction()
      let switcher = try await facadeViewer.toolSwitcher(for: facadeDocument)
      let facadeRead = try await step.viaFacade(Opened(target: switcher, viewer: facadeViewer, document: facadeDocument))
      let facadeState = try await observe(facadeInteraction)

      expectNoDifference(facadeRead, capabilityRead)
      expectNoDifference(facadeState, capabilityState)
      if capabilityRead.isEmpty {
        #expect(capabilityState != before, "\(step.name) should change the active tool")
      }
    }

    // ── errors ──

    @Test func theFacadesThrowWhatTheCapabilitiesThrow() async throws {
      let (viewer, document, stage) = try await openedStage()
      let navigator = try await viewer.pageNavigator(for: document)
      let zoom = try await viewer.pageZoom(for: document)
      let invalidInput: @Sendable (any Error) -> Bool = { ($0 as? PDFError)?.code == .invalidInput }
      await #expect(performing: { try await stage.goToPageIndex(-1) }, throws: invalidInput)
      await #expect(performing: { try await navigator.goToPageIndex(-1) }, throws: invalidInput)

      let (toolViewer, toolDocument, interaction) = try await openedInteraction()
      let switcher = try await toolViewer.toolSwitcher(for: toolDocument)
      let notFound: @Sendable (any Error) -> Bool = { ($0 as? PDFError)?.code == .notFound }
      await #expect(performing: { try await interaction.activateTool(ToolID("nobody-registered-this")) }, throws: notFound)
      await #expect(performing: { try await switcher.activateTool(ToolID("nobody-registered-this")) }, throws: notFound)

      try await viewer.close(document)
      let instanceClosed: @Sendable (any Error) -> Bool = { ($0 as? PDFError)?.code == .instanceClosed }
      await #expect(performing: { _ = try await stage.zoomLevel() }, throws: instanceClosed)
      await #expect(performing: { _ = try await zoom.zoomLevel() }, throws: instanceClosed)
      await #expect(performing: { _ = try await navigator.pageCount() }, throws: instanceClosed)
    }

    @Test func theAccessorsRefuseALeftOutPluginAsTheCapabilityAccessorsDo() async throws {
      let viewer = try PDFViewer(ViewerConfig())
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let unsupported: @Sendable (any Error) -> Bool = { ($0 as? PDFError)?.code == .unsupported }
      await #expect(performing: { _ = try await viewer.stage(for: document) }, throws: unsupported)
      await #expect(performing: { _ = try await viewer.pageNavigator(for: document) }, throws: unsupported)
      await #expect(performing: { _ = try await viewer.pageZoom(for: document) }, throws: unsupported)
      await #expect(performing: { _ = try await viewer.interaction(for: document) }, throws: unsupported)
      await #expect(performing: { _ = try await viewer.toolSwitcher(for: document) }, throws: unsupported)
      #expect(Self.accessors.count == 5)
    }
  }
}
