import EmbedPDF
import SwiftUI

/// A scrolling, zooming reader over a document's stage. Each visible page
/// sits where the stage put it and renders at the stage's render scale.
public struct StageReaderView: View {
  let model: StageReaderModel
  @Environment(\.displayScale) private var displayScale

  public init(model: StageReaderModel) {
    self.model = model
  }

  public var body: some View {
    GeometryReader { geometry in
      ZStack(alignment: .topLeading) {
        ForEach(model.visiblePages, id: \.page.pageObjectNumber) { page in
          PageImageView(
            render: model.render, page: page.page,
            scale: StageReaderModel.renderScale(
              page.transform.renderScale, contentWidth: page.transform.contentWidth,
              contentHeight: page.transform.contentHeight))
            .frame(width: page.transform.viewWidth, height: page.transform.viewHeight)
            .offset(x: page.screenX, y: page.screenY)
        }
      }
      .frame(width: geometry.size.width, height: geometry.size.height, alignment: .topLeading)
      .clipped()
      .contentShape(Rectangle())
      .gesture(
        DragGesture(minimumDistance: 0)
          .onChanged { value in Task { await model.dragChanged(translation: value.translation) } }
          .onEnded { value in Task { await model.dragEnded(velocity: value.velocity) } }
      )
      .simultaneousGesture(
        MagnifyGesture()
          .onChanged { value in model.magnifyChanged(value.magnification, at: value.startLocation) }
          .onEnded { _ in Task { await model.magnifyEnded() } }
      )
      // Simultaneous: the zero-distance drag above recognizes on touch-down
      // and would otherwise win every tap.
      .simultaneousGesture(
        SpatialTapGesture(count: 2, coordinateSpace: .local)
          .onEnded { value in Task { await model.doubleTapped(at: value.location) } }
      )
      .task { await model.task() }
      .task(id: geometry.size) { await model.sizeChanged(geometry.size, displayScale: displayScale) }
    }
  }
}
