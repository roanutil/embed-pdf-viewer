internal import EmbedPDFBindings

/// A registered tool, handler, command or locale. `unregister()` undoes it;
/// dropping this without calling it leaves the registration in place.
public struct Registration: Sendable {
  let handle: RegistrationHandle

  /// Removes the registration; repeated calls have no additional effect.
  ///
  /// Call this when the tool, handler, command, or locale leaves its owning
  /// application scope. Releasing the Swift value alone does not unregister it.
  public func unregister() {
    handle.unregister()
  }
}
