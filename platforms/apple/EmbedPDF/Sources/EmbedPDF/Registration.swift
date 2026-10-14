internal import EmbedPDFBindings

/// A registered tool, handler, command or locale. `unregister()` undoes it;
/// dropping this without calling it leaves the registration in place.
public struct Registration: Sendable {
  let handle: RegistrationHandle

  /// Idempotent.
  public func unregister() {
    handle.unregister()
  }
}
