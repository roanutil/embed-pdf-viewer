import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct PluginSettingsTests {
    @Test func selectionColorsClearAndSettingsReachBothDocuments() async throws {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, selection: true))
      try await viewer.start()
      let first = try await viewer.open(Fixture.data("report"))
      let second = try await viewer.open(Fixture.data("report"))
      let selection = try await viewer.selection(for: first)
      let other = try await viewer.selection(for: second)
      let registered = try await selection.settings()
      try await selection.updateSettings(SelectionSettingsPatch(
        dragThresholdPx: 11, color: "red", handles: SelectionHandleSettingsPatch(color: "blue", shadow: "none")))
      try await selection.updateSettings(SelectionSettingsPatch(clearColor: true))
      let cleared = try await other.settings()
      expectNoDifference(cleared.color, nil)
      expectNoDifference(cleared.dragThresholdPx, 11)
      expectNoDifference(cleared.handles.color, "blue")
      try await selection.updateSettings(SelectionSettingsPatch(handles: SelectionHandleSettingsPatch(clearColor: true)))
      let handleShadow = try await other.settings().handles.shadow
      expectNoDifference(handleShadow, "none")
      let handleColor = try await other.settings().handles.color
      expectNoDifference(handleColor, nil)
      try await other.resetSettings()
      let resetSelection = try await selection.settings()
      expectNoDifference(resetSelection, registered)
      try await viewer.close(first)
      await #expect { try await selection.settings() } throws: { ($0 as? PDFError)?.code == .instanceClosed }
      let survivingSelection = try await other.settings()
      expectNoDifference(survivingSelection, registered)
    }

    @Test func searchHighlightsAndNullableRevealMergeIndependently() async throws {
      let viewer = try PDFViewer(ViewerConfig(search: SearchConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let search = try await viewer.search(in: document)
      let registered = try await search.settings()
      try await search.updateSettings(SearchSettingsPatch(
        reveal: SearchRevealPatch(clearAnchor: true),
        highlight: SearchHighlightPatch(color: "red", blendMode: .screen)))
      let changed = try await search.settings()
      expectNoDifference(changed.reveal.anchor, nil)
      expectNoDifference(changed.reveal.behavior, registered.reveal.behavior)
      expectNoDifference(changed.highlight.activeColor, registered.highlight.activeColor)
      try await search.updateSettings(SearchSettingsPatch(highlight: SearchHighlightPatch(activeColor: "blue")))
      let highlightColor = try await search.settings().highlight.color
      expectNoDifference(highlightColor, "red")
      try await search.resetSettings()
      let resetSearch = try await search.settings()
      expectNoDifference(resetSearch, registered)
    }

    @Test func renderTileAndSnapshotPatchesPreserveOtherSettings() async throws {
      let viewer = try PDFViewer(ViewerConfig(render: RenderConfig(fullPage: RenderFullPageOptions(maxWidth: 480))))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let render = try await viewer.render(in: document)
      let registered = try await render.settings()
      try await render.updateSettings(RenderSettingsPatch(
        tiles: .off, slowAfterMs: 800,
        snapshot: RenderSnapshotPatch(scale: 3, boxLook: RenderSnapshotBox(fill: "red", stroke: "blue"))))
      try await render.updateSettings(RenderSettingsPatch(snapshot: RenderSnapshotPatch(boxLook: RenderSnapshotBox(fill: "green"))))
      let changed = try await render.settings()
      expectNoDifference(changed.fullPage.maxWidth, 480)
      expectNoDifference(changed.tiles, .off)
      expectNoDifference(changed.snapshot.scale, 3)
      expectNoDifference(changed.snapshot.boxLook.stroke, nil)
      try await render.resetSettings()
      let resetRender = try await render.settings()
      expectNoDifference(resetRender, registered)
    }

    @Test func formSettingsKeepValidationWhileClearingFocusAndFieldBorders() async throws {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, form: FormConfig(validation: FormValidation.none)))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let form = try await viewer.form(for: document)
      let registered = try await form.settings()
      try await form.updateSettings(FormSettingsPatch(
        focus: FormFocusSettingsPatch(color: "red"),
        fields: FormFieldSettingsPatch(border: "blue", background: "white"),
        fillable: .kinds(families: [.text, .checkbox])))
      try await form.updateSettings(FormSettingsPatch(
        focus: FormFocusSettingsPatch(clearColor: true), fields: FormFieldSettingsPatch(clearBorder: true)))
      let changed = try await form.settings()
      expectNoDifference(changed.validation, .none)
      expectNoDifference(changed.focus.color, nil)
      expectNoDifference(changed.fields.border, nil)
      expectNoDifference(changed.fields.background, "white")
      expectNoDifference(changed.fillable, .kinds(families: [.text, .checkbox]))
      try await form.resetSettings()
      let resetForm = try await form.settings()
      expectNoDifference(resetForm, registered)
    }

    @Test func localeSettingsPreserveLazyLoaderAcrossUpdateAndReset() async throws {
      let viewer = try PDFViewer(ViewerConfig(i18n: I18nConfig(
        locale: "en", locales: [I18nTests.english], lazyLocales: ["es"],
        loader: BundleLocaleLoader(bundle: .module, subdirectory: "Locales"))))
      try await viewer.start()
      let i18n = try await viewer.i18n()
      let registered = try await i18n.settings()
      try await i18n.updateSettings(I18nSettingsPatch(fallbackLocale: "es"))
      try await i18n.setLocale("es")
      let translatedPages = try await i18n.t("pages", params: ["count": .number(value: 2)])
      expectNoDifference(translatedPages, "2 páginas")
      try await i18n.updateSettings(I18nSettingsPatch(clearLocale: true))
      let clearedLocale = try await i18n.settings().locale
      expectNoDifference(clearedLocale, nil)
      try await i18n.resetSettings()
      let resetLocaleSettings = try await i18n.settings()
      expectNoDifference(resetLocaleSettings, registered)
    }

    @Test func signatureModeCanClearAndResetToRegisteredMode() async throws {
      let viewer = try PDFViewer(ViewerConfig(interaction: true, form: FormConfig(), signature: SignatureConfig(mode: .visual)))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      let signature = try await viewer.signatures(for: document)
      try await signature.updateSettings(SignatureSettingsPatch(clearMode: true, allowCertify: true))
      let changed = try await signature.settings()
      expectNoDifference(changed.mode, nil)
      #expect(changed.allowCertify)
      try await signature.resetSettings()
      let resetSignatureMode = try await signature.settings().mode
      expectNoDifference(resetSignatureMode, .visual)
      let resetSignature = try await signature.settings()
      #expect(!resetSignature.allowCertify)
    }

    @Test func commandSettingsValidateOpenAppDataAndReset() async throws {
      let viewer = try PDFViewer(ViewerConfig(commands: true))
      try await viewer.start()
      let commands = try await viewer.commands()
      let registered = try await commands.settings()
      try await commands.updateSettings(CommandsSettingsPatch(
        disabledCategories: ["edit"], disabledCommands: ["print"], notAllowed: .disable, appStateJson: "null"))
      let changed = try await commands.settings()
      expectNoDifference(changed.appStateJson, "null")
      await #expect(throws: PDFError.self) {
        try await commands.updateSettings(CommandsSettingsPatch(appStateJson: "{"))
      }
      let settingsAfterInvalidPatch = try await commands.settings()
      expectNoDifference(settingsAfterInvalidPatch, changed)
      try await commands.resetSettings()
      let resetCommands = try await commands.settings()
      expectNoDifference(resetCommands, registered)
    }

    @Test func stampResetRestoresRegisteredPreviewWidth() async throws {
      let viewer = try PDFViewer(ViewerConfig(stamp: StampConfig(previewWidth: 320, dynamic: true)))
      try await viewer.start()
      let stamps = try await viewer.stamps()
      try await stamps.updateSettings(StampConfig(dynamic: false))
      let previewWidth = try await stamps.settings().previewWidth
      expectNoDifference(previewWidth, 320)
      try await stamps.resetSettings()
      let resetStamp = try await stamps.settings()
      #expect(resetStamp.dynamic)
    }

    @Test func interactionDefaultAppliesToNewDocuments() async throws {
      let viewer = try PDFViewer(ViewerConfig(interaction: true))
      try await viewer.start()
      let first = try await viewer.open(Fixture.data("report"))
      let interaction = try await viewer.interaction(for: first)
      let registered = try await interaction.settings()
      try await interaction.updateSettings(InteractionSettingsPatch(defaultTool: "pan"))
      let second = try await viewer.open(Fixture.data("report"))
      let other = try await viewer.interaction(for: second)
      let newDocumentTool = try await other.activeToolId()
      expectNoDifference(newDocumentTool, "pan")
      try await other.resetSettings()
      let resetInteraction = try await interaction.settings()
      expectNoDifference(resetInteraction, registered)
    }
  }
}
