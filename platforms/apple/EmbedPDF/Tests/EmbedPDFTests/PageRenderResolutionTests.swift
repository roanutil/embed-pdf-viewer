import EmbedPDF
import Testing

@Suite struct PageRenderResolutionTests {
  @Test func invisiblePagesIgnoreZoomAndKeepSmallPlaceholders() {
    let result = PageRenderResolution.forPage(viewWidth: 800, displayScale: 3, zoomScale: 8, isOnScreen: false)
    #expect(result.deviceWidth == 256)
    #expect(!result.isFull)
  }

  @Test func visiblePagesRoundUpToPixelBands() {
    let result = PageRenderResolution.forPage(viewWidth: 390, displayScale: 2, zoomScale: 2, isOnScreen: true)
    #expect(result.deviceWidth == 1664)
    #expect(result.isFull)
  }

  @Test func zoomingOutDoesNotLowerTheNativeDisplayDemand() {
    #expect(PageRenderResolution.forPage(viewWidth: 390, displayScale: 2, zoomScale: 0.25, isOnScreen: true).deviceWidth == 896)
  }

  @Test func anEmptyLayoutKeepsTheMinimumRasterDemand() {
    #expect(PageRenderResolution.forPage(viewWidth: 0, displayScale: 2, isOnScreen: true).deviceWidth == 64)
  }

  @Test func aQuarterTurnConvertsDisplayedWidthBackToPageWidth() {
    let result = PageRenderResolution(deviceWidth: 1024, isFull: true)
    #expect(result.unrotatedDeviceWidth(unrotatedWidth: 612, displayedWidth: 792) == 791)
  }
}
