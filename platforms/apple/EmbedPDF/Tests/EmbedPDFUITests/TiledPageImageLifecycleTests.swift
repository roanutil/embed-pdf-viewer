#if canImport(UIKit)
import EmbedPDF
@testable import EmbedPDFUI
import Foundation
import Observation
import SwiftUI
import Testing
import UIKit

@MainActor
@Suite(.serialized, .timeLimit(.minutes(1)))
struct TiledPageImageLifecycleTests {
  @Observable
  final class Selection {
    var model: TiledPageModel
    init(_ model: TiledPageModel) { self.model = model }
  }

  private struct SelectedPage: View {
    let selection: Selection
    let size: CGSize

    var body: some View {
      // Keep the structural position, scale, rect and revision unchanged.
      TiledPageImage(model: selection.model, unrotatedSize: size, deviceScale: 8, visibleRect: nil)
    }
  }

  private func waitUntil(_ condition: () async throws -> Bool) async throws -> Bool {
    let clock = ContinuousClock()
    let deadline = clock.now + .seconds(10)
    while clock.now < deadline {
      if try await condition() { return true }
      try await Task.sleep(for: .milliseconds(10))
    }
    return false
  }

  @Test func replacingTheModelWithEqualDemandUpdatesTheNewPageAndReleasesTheOldPage() async throws {
    let viewer = try PDFViewer(ViewerConfig(render: RenderConfig()))
    try await viewer.start()
    do {
      let url = try #require(Bundle.module.url(forResource: "report", withExtension: "pdf", subdirectory: "Fixtures"))
      let document = try await viewer.open(Data(contentsOf: url))
      let pages = try await viewer.pages(of: document)
      try #require(pages.count >= 2)
      let render = try await viewer.render(in: document)
      let demand = try await render.viewDemand("c8-model-replacement")
      let old = TiledPageModel(render: render, demand: demand, page: pages[0].page)
      let new = TiledPageModel(render: render, demand: demand, page: pages[1].page)
      let selection = Selection(old)
      let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 320, height: 480))
      let host = UIHostingController(rootView: SelectedPage(
        selection: selection, size: CGSize(width: pages[0].width, height: pages[0].height)))
      window.rootViewController = host
      window.makeKeyAndVisible()
      host.view.layoutIfNeeded()
      do {
        let oldWasRendered = try await waitUntil { old.base != nil && !old.tiles.isEmpty }
        try #require(oldWasRendered)
        let oldPlan = try await demand.plan(old.page)
        try #require(oldPlan.engaged)

        selection.model = new

        let newWasRendered = try await waitUntil { new.base != nil }
        #expect(newWasRendered)
        let oldWasReleased = try await waitUntil {
          let plan = try await demand.plan(old.page)
          return !plan.engaged && plan.paint.isEmpty && plan.fetching.isEmpty && old.tiles.isEmpty
        }
        #expect(oldWasReleased)
      } catch {
        window.isHidden = true
        window.rootViewController = nil
        await old.release()
        await new.release()
        try? await demand.dispose()
        throw error
      }
      window.isHidden = true
      window.rootViewController = nil
      await old.release()
      await new.release()
      try await demand.dispose()
    } catch {
      try? await viewer.destroy()
      throw error
    }
    try await viewer.destroy()
  }
}
#endif
