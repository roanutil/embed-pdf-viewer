public import EmbedPDFBindings
import Foundation
import JavaScriptCore

public typealias ScriptEngine = EmbedPDFBindings.ScriptEngine
public typealias ScriptEngineFactory = EmbedPDFBindings.ScriptEngineFactory
public typealias ScriptEvaluation = EmbedPDFBindings.ScriptEvaluation
public typealias ScriptCheck = EmbedPDFBindings.ScriptCheck

/// PDF JavaScript in JavaScriptCore, one `JSContext` per realm. Pass it as
/// `ActionsJavaScriptConfig(enabled: true, engine: JavaScriptCoreEngineFactory())`.
///
/// JavaScriptCore has no public execution time limit, so a realm that runs
/// past its budget is abandoned: the call returns `timedOut`, the realm is
/// poisoned, and the runaway script keeps its thread busy until the process
/// exits. Memory and stack budgets aren't enforced; JavaScriptCore's own
/// stack limit applies.
public final class JavaScriptCoreEngineFactory: ScriptEngineFactory, Sendable {
  public init() {}

  public func create() throws -> any ScriptEngine {
    JavaScriptCoreEngine()
  }
}

final class JavaScriptCoreEngine: ScriptEngine, @unchecked Sendable {
  private let queue: DispatchQueue
  private let lock = NSLock()
  private var abandoned = false
  /// Touched only on `queue`.
  private var context: JSContext?

  /// `target` is for tests that need the realm's queue to start late.
  init(target: DispatchQueue? = nil) {
    queue = DispatchQueue(label: "com.embedpdf.acrojs", qos: .userInitiated, target: target)
    queue.sync { context = JSContext() }
  }

  func evaluate(program: String, timeLimitMs: UInt32) -> ScriptEvaluation {
    if lock.withLock({ abandoned }) {
      return .threw(detail: "the realm was abandoned after a timeout")
    }
    let started = DispatchSemaphore(value: 0)
    let done = DispatchSemaphore(value: 0)
    let answer = Answer()
    queue.async { [weak self] in
      started.signal()
      defer { done.signal() }
      guard let context = self?.context else {
        answer.set(.threw(detail: "the realm is disposed"))
        return
      }
      context.exception = nil
      let value = context.evaluateScript(program)
      if let exception = context.exception {
        answer.set(.threw(detail: exception.toString() ?? "an exception"))
      } else if let value, value.isString, let json = value.toString() {
        answer.set(.value(json: json))
      } else {
        answer.set(.threw(detail: "the program did not evaluate to a string"))
      }
    }
    // The budget is the script's, not the queue's: the clock starts when
    // the block does. The queue only ever runs this realm's work, and a
    // realm that timed out is abandoned above, so the start always comes.
    started.wait()
    if done.wait(timeout: .now() + .milliseconds(Int(timeLimitMs))) == .timedOut {
      lock.withLock { abandoned = true }
      return .timedOut
    }
    return answer.get() ?? .threw(detail: "no answer")
  }

  func dispose() {
    lock.withLock { abandoned = true }
    queue.async { [weak self] in self?.context = nil }
  }

  private final class Answer: @unchecked Sendable {
    private let lock = NSLock()
    private var value: ScriptEvaluation?
    func set(_ value: ScriptEvaluation) { lock.withLock { self.value = value } }
    func get() -> ScriptEvaluation? { lock.withLock { value } }
  }
}

public enum ScriptEngineCheck {
  /// Runs `source` once in a fresh realm from `factory`, outside any
  /// document, with the default budget.
  public static func run(_ factory: any ScriptEngineFactory, source: String) async throws(PDFError) -> ScriptCheck {
    let task = Task.detached { () throws -> ScriptCheck in
      try checkScriptEngine(factory: factory, source: source)
    }
    do {
      return try await task.value
    } catch {
      throw PDFError(error)
    }
  }
}
