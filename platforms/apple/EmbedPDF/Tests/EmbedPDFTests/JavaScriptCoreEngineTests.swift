import CustomDump
import Foundation
@testable import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct JavaScriptCoreEngineTests {
    @Test func anAlertComesBackAsAnEffect() async throws {
      let check = try await ScriptEngineCheck.run(JavaScriptCoreEngineFactory(), source: "app.alert('hi')")
      expectNoDifference(check.alerts, ["hi"])
      expectNoDifference(check.error, nil)
      expectNoDifference(check.disposed, false)
    }

    @Test func aThrowIsAnExceptionAndKeepsTheRealm() async throws {
      let check = try await ScriptEngineCheck.run(JavaScriptCoreEngineFactory(), source: "throw new Error('boom')")
      expectNoDifference(check.error?.kind, .exception)
      expectNoDifference(check.disposed, false)
    }

    @Test func aRunawayScriptTimesOut() async throws {
      let clock = ContinuousClock()
      let start = clock.now
      let check = try await ScriptEngineCheck.run(JavaScriptCoreEngineFactory(), source: "while (true) {}")
      expectNoDifference(check.error?.kind, .budget)
      expectNoDifference(check.disposed, true)
      // The default budget is 50 ms; allow scheduling slack, not a hang.
      #expect(clock.now - start < .seconds(2))
    }

    @Test func aLateStartingQueueDoesNotCountAgainstTheBudget() {
      let busy = DispatchQueue(label: "busy")
      let engine = JavaScriptCoreEngine(target: busy)
      busy.async { Thread.sleep(forTimeInterval: 0.15) }
      let answer = engine.evaluate(program: "JSON.stringify(1)", timeLimitMs: 50)
      expectNoDifference(answer, .value(json: "1"))
    }

    @Test func aDeepRecursionIsABudgetFailure() async throws {
      let check = try await ScriptEngineCheck.run(
        JavaScriptCoreEngineFactory(), source: "function f() { return f() + 1 } f()"
      )
      expectNoDifference(check.error?.kind, .budget)
    }
  }
}
