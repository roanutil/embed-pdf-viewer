#if canImport(AppKit)
import AppKit
import CustomDump
import EmbedPDF
@testable import EmbedPDFUI
import Observation
import SwiftUI
import Testing

@MainActor
@Suite(.serialized, .timeLimit(.minutes(1)))
struct PanelPresentationTests {
  @Observable
  final class Harness {
    var model: OutlineModel
    var renderedID: ObjectIdentifier?
    var started: Set<ObjectIdentifier> = []
    var ended: Set<ObjectIdentifier> = []
    @ObservationIgnored var open: (@MainActor () -> Void)?
    @ObservationIgnored var save: (@MainActor () async -> Void)?
    @ObservationIgnored var delete: (@MainActor () async -> Void)?
    init(_ model: OutlineModel) { self.model = model }
  }

  private struct Pending {
    let ref: OutlineItemRef
    let title: String
  }

  private struct InspectorProbe: View {
    let model: OutlineModel
    let harness: Harness
    @State private var pending: Pending?
    @State private var active = true

    var body: some View {
      Color.clear
        .onAppear { active = true }
        .onDisappear { active = false }
        .onChange(of: ObjectIdentifier(model), initial: true) { _, id in installActions(id) }
        .task { await observeLifetime() }
    }

    private func installActions(_ id: ObjectIdentifier) {
      harness.open = {
        if let item = model.items.first { pending = Pending(ref: item.itemRef, title: item.title) }
      }
      harness.save = {
        guard active, let pending else { return }
        _ = await model.update(pending.ref, OutlineItemPatch(title: pending.title))
      }
      harness.delete = {
        guard active, let pending else { return }
        await model.delete([pending.ref])
      }
      harness.renderedID = id
    }

    private func observeLifetime() async {
      let id = ObjectIdentifier(model)
      harness.started.insert(id)
      defer { harness.ended.insert(id) }
      let lifetime = AsyncStream<Void> { _ in }
      for await _ in lifetime {}
    }
  }

  private struct Host: View {
    let harness: Harness
    var body: some View {
      ModelScope(model: harness.model) { InspectorProbe(model: $0, harness: harness) }
    }
  }

  @Test func modelReplacementClearsPendingActionsAndCancelsTheOldLifetime() async throws {
    _ = NSApplication.shared
    let viewer = try PDFViewer(ViewerConfig(outline: true))
    try await viewer.start()
    do {
      let first = try await viewer.open(pluginFixture())
      let second = try await viewer.open(pluginFixture())
      let firstModel = OutlineModel(viewer: viewer, outline: try await viewer.outline(for: first))
      let secondModel = OutlineModel(viewer: viewer, outline: try await viewer.outline(for: second))
      await firstModel.reload(); await secondModel.reload()
      let firstItem = try #require(await firstModel.create(OutlineItemInput(title: "First", clearTarget: true)))
      let secondItem = try #require(await secondModel.create(OutlineItemInput(title: "Second", clearTarget: true)))
      expectNoDifference(firstItem.itemRef, secondItem.itemRef)
      let harness = Harness(firstModel)
      let host = NSHostingView(rootView: Host(harness: harness))
      let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 320, height: 240),
        styleMask: [.titled], backing: .buffered, defer: false)
      window.isReleasedWhenClosed = false
      window.contentView = host
      defer { window.orderOut(nil); window.contentView = nil; window.close() }
      try await expectObserved({ harness.renderedID }, equals: ObjectIdentifier(firstModel)) {
        window.makeKeyAndOrderFront(nil)
        host.layoutSubtreeIfNeeded()
      }
      try await expectObserved({ harness.started.contains(ObjectIdentifier(firstModel)) }, equals: true) {}
      let open = try #require(harness.open)
      open()
      let oldSave = try #require(harness.save)
      let oldDelete = try #require(harness.delete)
      try await expectObserved({ harness.renderedID }, equals: ObjectIdentifier(secondModel)) { harness.model = secondModel }
      await harness.save?()
      expectNoDifference(secondModel.items.map(\.title), ["Second"])
      await harness.delete?()
      expectNoDifference(secondModel.items.map(\.title), ["Second"])
      await oldSave()
      await oldDelete()
      expectNoDifference(firstModel.items.map(\.title), ["First"])
      try await expectObserved({ harness.ended.contains(ObjectIdentifier(firstModel)) }, equals: true) {}
    } catch {
      try? await viewer.destroy()
      throw error
    }
    try await viewer.destroy()
  }
}
#endif
