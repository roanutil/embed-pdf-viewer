import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct I18nTests {
    static let english = LocalePack(
      code: "en", name: "English", dir: nil,
      translations: ["hello": "Hello", "greet": "Hi {name}"]
    )

    func started(lazy: [String] = ["es"], subdirectory: String = "Locales")
      async throws -> (PDFViewer, I18nCapability)
    {
      let loader = BundleLocaleLoader(bundle: .module, subdirectory: subdirectory)
      let viewer = try PDFViewer(
        ViewerConfig(
          i18n: I18nConfig(locale: "en", locales: [Self.english], lazyLocales: lazy, loader: loader)
        )
      )
      try await viewer.start()
      return (viewer, try await viewer.i18n())
    }

    @Test func translatesAndInterpolates() async throws {
      let (_, i18n) = try await started()
      let hello = try await i18n.t("hello")
      expectNoDifference(hello, "Hello")
      let greet = try await i18n.t("greet", params: ["name": .text(value: "Ana")])
      expectNoDifference(greet, "Hi Ana")
      let missing = try await i18n.t("nope", fallback: "Fallback")
      expectNoDifference(missing, "Fallback")
    }

    @Test func aLazySwitchReadsTheBundleAndAnnounces() async throws {
      let (_, i18n) = try await started()
      let events = i18n.events
      try await i18n.setLocale("es")
      for await event in events {
        guard case let .localeChanged(event) = event else { continue }
        expectNoDifference(event.locale, "es")
        expectNoDifference(event.previousLocale, "en")
        break
      }
      let pages = try await i18n.t("pages", params: ["count": .number(value: 2)])
      expectNoDifference(pages, "2 páginas")
      let locales = try await i18n.locales()
      expectNoDifference(locales.map(\.code), ["en", "es"])
    }

    @Test func aMissingPackFailsTheSwitch() async throws {
      let (_, i18n) = try await started(subdirectory: "Nowhere")
      await #expect {
        try await i18n.setLocale("es")
      } throws: { error in
        (error as? PDFError)?.code == "operation-failed"
      }
      let locale = try await i18n.locale()
      expectNoDifference(locale, "en")
    }

    @Test func anUnknownLocaleIsNotFound() async throws {
      let (_, i18n) = try await started()
      await #expect {
        try await i18n.setLocale("fr")
      } throws: { error in
        (error as? PDFError)?.code == "not-found"
      }
    }

    @Test func aRegisteredPackIsUsableAndRemovable() async throws {
      let (_, i18n) = try await started()
      let registration = try await i18n.register(
        LocalePack(code: "de", name: "Deutsch", dir: nil, translations: ["hello": "Hallo"])
      )
      try await i18n.setLocale("de")
      let hello = try await i18n.t("hello")
      expectNoDifference(hello, "Hallo")
      try await i18n.setLocale("en")
      registration.unregister()
      await #expect(throws: PDFError.self) { try await i18n.setLocale("de") }
    }

    @Test func aConfigWithALoaderEqualsItself() {
      // ViewerConfig isn't Equatable since FeedbackConfig became an object
      // (plan 01), so this checks the I18nConfig the export covers.
      let config = I18nConfig(lazyLocales: ["es"], loader: BundleLocaleLoader(bundle: .module))
      #expect(config == config)
      expectNoDifference(config.hashValue, config.hashValue)
    }

    @Test func aLocaleFileDecodes() throws {
      let url = try #require(Bundle.module.url(forResource: "es", withExtension: "json", subdirectory: "Locales"))
      let pack = try LocalePack.decode(json: String(contentsOf: url, encoding: .utf8))
      expectNoDifference(pack.translations["pages.other"], "{count} páginas")
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, i18n) = try await started()
      let task = Task { for await _ in i18n.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      try await i18n.setLocale("es")
    }

    @Test func aDestroyedViewersCapabilityThrows() async throws {
      let (viewer, i18n) = try await started()
      try await viewer.destroy()
      await #expect {
        _ = try await i18n.locale()
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }
  }
}
