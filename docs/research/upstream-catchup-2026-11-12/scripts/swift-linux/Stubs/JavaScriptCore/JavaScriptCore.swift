// JavaScriptCore on Linux: a context that evaluates nothing.
import Foundation

public final class JSValue {
  public var isString: Bool { false }
  public func toString() -> String? { nil }
}

public final class JSContext {
  public var exception: JSValue?
  public init() {}
  public func evaluateScript(_ script: String) -> JSValue? {
    exception = JSValue()
    return nil
  }
}
