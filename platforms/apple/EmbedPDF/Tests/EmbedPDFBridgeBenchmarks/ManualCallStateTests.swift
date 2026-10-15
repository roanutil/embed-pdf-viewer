import EmbedPDFManualBridge
import Testing

/// A cancel before the C call answers its id used to be lost: `onCancel`
/// found no id and did nothing, and the call ran to the end.
@Suite struct ManualCallStateTests {
  @Test func aCancelBeforeTheCallHasItsIdCancelsOnceItDoes() {
    var state = ManualCallState.starting
    let early = state.cancel()
    #expect(early == nil)
    let cancelNow = state.start(7)
    #expect(cancelNow)
    // A second cancel, from a task cancelled twice, finds nothing to do.
    let again = state.cancel()
    #expect(again == nil)
  }

  @Test func aCancelAfterTheCallHasItsIdCancelsIt() {
    var state = ManualCallState.starting
    let cancelNow = state.start(7)
    #expect(!cancelNow)
    let id = state.cancel()
    #expect(id == 7)
  }
}
