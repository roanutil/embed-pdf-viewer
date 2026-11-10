/// Whether a call includes a part the session may not be allowed to read:
/// a search hit's snippet, or a page picture's annotations or form fields.
///
/// The engine's default is ``whenPermitted``, which no `Bool` can say: what
/// it includes follows the session's scope, so the same call draws
/// annotations in one viewer and leaves them out in another.
public enum ContentInclusion: Sendable, Hashable {
  /// Include the part when the session may read it, and leave it out
  /// otherwise. The engine's default.
  case whenPermitted
  /// Include the part. A call that produces it throws `permission-denied`
  /// when the session may not read it.
  case always
  /// Leave the part out.
  case never

  /// The engine's spelling: `nil` is ``whenPermitted``.
  var engineValue: Bool? {
    switch self {
    case .whenPermitted: nil
    case .always: true
    case .never: false
    }
  }
}
