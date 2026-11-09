import CustomDump
@testable import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct RevisionHubTests {
    @Test func deliveryCoalescesIndependentlyAndFinishesEveryObserver() async {
      let hub = StateRevisionHub()
      let first = hub.stream()
      let second = hub.stream()
      hub.yield(1)
      hub.yield(2)
      var firstIterator = first.makeAsyncIterator()
      var secondIterator = second.makeAsyncIterator()
      let firstValue = await firstIterator.next()
      let secondValue = await secondIterator.next()
      expectNoDifference(firstValue, 2)
      expectNoDifference(secondValue, 2)
      let cancelled = Task { for await _ in hub.stream() {} }
      cancelled.cancel()
      await cancelled.value
      hub.yield(3)
      let continued = await secondIterator.next()
      expectNoDifference(continued, 3)
      hub.finish()
      for await _ in first {}
      for await _ in second {}
      var late = hub.stream().makeAsyncIterator()
      let end = await late.next()
      expectNoDifference(end, nil)
    }

    @Test func repeatedCancellationLeavesTheSurvivingObserverLive() async {
      let hub = StateRevisionHub()
      var survivor = hub.stream().makeAsyncIterator()
      for revision in UInt64(1)...20 {
        let disposable = Task { for await _ in hub.stream() {} }
        hub.yield(revision)
        disposable.cancel()
        await disposable.value
        let current = await survivor.next()
        expectNoDifference(current, revision)
      }
      hub.finish()
      let end = await survivor.next()
      expectNoDifference(end, nil)
    }

    @Test func droppingTheHubFinishesAnUnconsumedStream() async {
      var hub: StateRevisionHub? = StateRevisionHub()
      var iterator = hub!.stream().makeAsyncIterator()
      hub = nil
      let end = await iterator.next()
      expectNoDifference(end, nil)
    }
  }
}
