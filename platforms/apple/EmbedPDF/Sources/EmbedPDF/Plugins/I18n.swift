public import EmbedPDFBindings
import Foundation

/// The initial locale, fallback, packs, and lazy loader used for viewer translations.
public typealias I18nConfig = EmbedPDFBindings.I18nConfig
/// A locale's code, name, text direction, and flat translation keys.
public typealias LocalePack = EmbedPDFBindings.LocalePack
/// The display information for a registered or lazily available locale.
public typealias LocaleInfo = EmbedPDFBindings.LocaleInfo
/// The reading direction used by a locale's interface text.
public typealias TextDirection = EmbedPDFBindings.TextDirection
/// A string or number used for translation interpolation and plural selection.
public typealias I18nParam = EmbedPDFBindings.I18nParam
/// An event describing locale changes and lazy-load failures.
public typealias I18nEvent = EmbedPDFBindings.I18nEvent
/// An event describing a completed locale switch.
public typealias LocaleChangedEvent = EmbedPDFBindings.LocaleChangedEvent
/// The locale code and error information for a failed lazy load.
public typealias LocaleLoadFailure = EmbedPDFBindings.LocaleLoadFailure
/// The host callback that loads a requested locale and completes with a pack or failure.
public typealias LocaleLoader = EmbedPDFBindings.LocaleLoader
/// The callback a locale loader must finish or fail.
public typealias LocaleLoadCompletion = EmbedPDFBindings.LocaleLoadCompletion

extension LocalePack {
  /// Decodes a locale file in the web's shape (`{code, name, dir?, translations}`).
  ///
  /// - Parameter json: JSON text containing locale metadata and translations.
  /// - Throws: A `PDFError` when the JSON is invalid or does not describe a locale pack.
  public static func decode(json: String) throws(PDFError) -> LocalePack {
    do {
      return try localePackFromJson(json: json)
    } catch {
      throw PDFError(error)
    }
  }
}

/// The viewer's locale packs and translations. There is one per viewer and
/// it works before any document opens. Get it with `PDFViewer.i18n()`; the
/// viewer needs `ViewerConfig(i18n: I18nConfig(...))`.
public struct I18nCapability: Sendable {
  let handle: I18nHandle
  let viewer: PDFViewer

  /// The current locale, then the fallback locale, then `fallback`, then the
  /// key itself. A `.number` named `count` picks a plural form.
  public func t(_ key: String, params: [String: I18nParam] = [:], fallback: String? = nil)
    async throws(PDFError) -> String
  {
    try await bridged { try await handle.t(key: key, params: params, fallback: fallback) }
  }

  /// Returns whether the current or fallback locale translates the key, without a missing-key warning.
  public func hasKey(_ key: String) async throws(PDFError) -> Bool {
    try await bridged { try await handle.hasKey(key: key) }
  }

  /// Returns the code of the currently active locale.
  public func locale() async throws(PDFError) -> String {
    try await bridged { try await handle.locale() }
  }

  /// Returns the active locale's text direction for interface layout.
  public func direction() async throws(PDFError) -> TextDirection {
    try await bridged { try await handle.direction() }
  }

  /// Registered packs, then lazy ones not loaded yet.
  public func locales() async throws(PDFError) -> [LocaleInfo] {
    try await bridged { try await handle.locales() }
  }

  /// Returns the code of the locale currently loading, or nil when no lazy switch is pending.
  public func loadingLocale() async throws(PDFError) -> String? {
    try await bridged { try await handle.loadingLocale() }
  }

  /// Returns once the locale is usable. Throws `not-found` for an unknown
  /// code, `operation-failed` when the pack fails to load and
  /// `operation-cancelled` when a newer call supersedes this one.
  /// Cancelling the task doesn't cancel the switch.
  public func setLocale(_ code: String) async throws(PDFError) {
    try await bridged { try await handle.setLocale(code: code) }
  }

  /// Dropping the registration keeps the pack; `unregister()` removes it.
  public func register(_ pack: LocalePack) async throws(PDFError) -> Registration {
    Registration(handle: try await bridged { try await handle.registerLocale(pack: pack) })
  }

  /// Flat keys (`"zoom.in"`); later keys win.
  public func addTranslations(_ translations: [String: String], to code: String)
    async throws(PDFError)
  {
    try await bridged { try await handle.addTranslations(code: code, translations: translations) }
  }

  /// Finishes when the viewer is destroyed.
  public var events: AsyncStream<I18nEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: I18nEventListener where Event == I18nEvent {
  func onEvent(event: I18nEvent) {
    continuation.yield(event)
  }
}

extension PDFViewer {
  /// Returns the workspace translation capability; configure i18n before requesting it.
  public func i18n() async throws(PDFError) -> I18nCapability {
    let handle = try await bridged { try await host.i18n() }
    return I18nCapability(handle: handle, viewer: self)
  }
}

extension LocaleLoadCompletion {
  /// Fail the load with `error`, keeping its code (see
  /// `HostFailure.init(_:)`) in `.localeLoadFailed`.
  public func fail(_ error: any Error) {
    failWith(failure: HostFailure(error))
  }
}

/// Serves lazy packs from `<code>.json` files in a bundle, in the web's
/// locale file shape. Reads off the kernel thread.
public final class BundleLocaleLoader: LocaleLoader, @unchecked Sendable {
  let bundle: Bundle
  let subdirectory: String?

  /// Creates a lazy loader that reads locale-code JSON files from the supplied bundle and
  /// optional resource subdirectory.
  public init(bundle: Bundle = .main, subdirectory: String? = nil) {
    self.bundle = bundle
    self.subdirectory = subdirectory
  }

  /// Reads code.json on a background queue and finishes with its decoded pack, or fails the
  /// completion if the resource is missing or invalid.
  public func load(code: String, completion: LocaleLoadCompletion) {
    let (bundle, subdirectory) = (bundle, subdirectory)
    DispatchQueue.global(qos: .userInitiated).async {
      guard let url = bundle.url(forResource: code, withExtension: "json", subdirectory: subdirectory) else {
        completion.fail(detail: "no \(code).json in \(bundle.bundlePath)/\(subdirectory ?? "")")
        return
      }
      do {
        let json = try String(contentsOf: url, encoding: .utf8)
        completion.finish(pack: try LocalePack.decode(json: json))
      } catch {
        completion.fail(error)
      }
    }
  }
}
