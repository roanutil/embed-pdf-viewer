import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct PageRenderResolutionTests {
    @Test func anOffScreenPageGetsThePlaceholderWidth() {
      expectNoDifference(
        PageRenderResolution.forPage(viewWidth: 1024, displayScale: 2, isOnScreen: false),
        PageRenderResolution(deviceWidth: PageRenderResolution.offScreenDeviceWidth, isFull: false))
    }

    @Test func anOnScreenPageRoundsUpToTheStep() {
      expectNoDifference(
        PageRenderResolution.forPage(viewWidth: 1000, displayScale: 2, isOnScreen: true),
        PageRenderResolution(deviceWidth: 2048, isFull: true))
    }

    @Test func zoomScalesTheWidthButNeverShrinksIt() {
      expectNoDifference(
        PageRenderResolution.forPage(viewWidth: 512, displayScale: 2, zoomScale: 2, isOnScreen: true).deviceWidth,
        2048)
      expectNoDifference(
        PageRenderResolution.forPage(viewWidth: 512, displayScale: 2, zoomScale: 0.5, isOnScreen: true).deviceWidth,
        1024)
    }

    @Test func aQuarterTurnedPageMeasuresAcrossItsUnrotatedWidth() {
      let resolution = PageRenderResolution(deviceWidth: 1584, isFull: true)
      expectNoDifference(resolution.unrotatedDeviceWidth(unrotatedWidth: 612, displayedWidth: 792), 1224)
    }
  }
}
