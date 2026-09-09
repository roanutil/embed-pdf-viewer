/**
 * The stamps sidebar (right panel). The stamp plugin owns the libraries, the
 * asset bytes and the cached previews; this panel is the PICKER over them:
 *
 *   click a stamp → `armAsset` arms the annotation plugin's stamp tool with
 *   that asset's bytes → hovering a page ghosts the exact placement → each
 *   click on a page places one.
 *
 * Deliberately NOT a file dialog: picking arbitrary image bytes is the Image
 * tool's job (a click-then-pick tool, `insert:add-image`). The stamp button
 * opens a library; a library is a set of reusable, named assets — the two
 * gestures are different, so they are different controls.
 *
 * The panel stays open while you place, so a stamp can be dropped on several
 * pages and swapped without a round trip through the toolbar.
 */
import { useEffect, useRef, useState } from 'react';
import { useTool } from '@embedpdf/react/interaction';
import {
  indexedDbByteStore,
  persistStampLibraries,
  restoreStampLibraries,
  useArmStampAsset,
  useStamp,
  useStampAssetPreviewUrl,
  useStampAssets,
  useStampLibraries,
  type StampAsset,
} from '@embedpdf/react/stamp';
import { useT } from '@embedpdf/react/i18n';
import { DEFAULT_LIBRARY_ID, seedDefaultStamps } from '../config/default-stamps';
import { Icon } from './icons';

/** The user's libraries live in the browser (IndexedDB): restored on first
 *  open, written on every change. The built-in set is never stored — it is
 *  drawn again next time, in the locale of that moment. */
const store =
  typeof indexedDB === 'undefined'
    ? null
    : indexedDbByteStore('embedpdf-stamps', { storeName: 'libraries' });

export function StampsPanel() {
  const t = useT();
  const stamp = useStamp();
  const libraries = useStampLibraries();
  const assets = useStampAssets();
  const { armAsset } = useArmStampAsset();
  const { activeToolId } = useTool();
  const fileRef = useRef<HTMLInputElement>(null);
  const [armedId, setArmedId] = useState<string | null>(null);
  const [busy, setBusy] = useState<'seeding' | 'importing' | null>(null);
  const [error, setError] = useState<string | null>(null);

  // First open: bring the user's stored libraries back, THEN draw the
  // built-in set if there is still nothing — never at viewer boot, so a
  // viewer whose user never opens this panel pays nothing for it. From then
  // on every change to a user library is written back.
  useEffect(() => {
    let live = true;
    setBusy('seeding');
    const restore = store ? restoreStampLibraries(stamp, store) : Promise.resolve([]);
    restore
      .then(() => seedDefaultStamps(stamp, t('demo.stampsStandard')))
      .catch((err: unknown) => {
        console.error('[embedpdf] default stamps failed:', err);
        if (live) setError(t('demo.stampsError'));
      })
      .finally(() => live && setBusy(null));
    const stopPersisting = store
      ? persistStampLibraries(stamp, store, { except: [DEFAULT_LIBRARY_ID] })
      : () => {};
    return () => {
      live = false;
      stopPersisting();
    };
    // Init-only: the library is seeded once per workspace (the locale at that
    // moment names it), not re-seeded when the locale changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stamp]);

  // The interaction hub owns tool state: leaving the stamp tool (picking
  // another tool, pressing Escape) un-highlights the gallery.
  useEffect(() => {
    if (activeToolId !== 'stamp') setArmedId(null);
  }, [activeToolId]);

  const arm = (asset: StampAsset) => {
    setError(null);
    setArmedId(asset.id);
    void armAsset(asset.id).catch((err) => {
      console.error('[embedpdf] arm stamp failed:', err);
      setArmedId(null);
      setError(t('demo.stampsArmError'));
    });
  };

  const importPdf = (file: File) => {
    setError(null);
    setBusy('importing');
    // The file name is only a FALLBACK: an Acrobat-authored or previously
    // exported library names itself through its /Title.
    stamp
      .importLibraryPdf(file, { name: file.name.replace(/\.pdf$/i, '') })
      .catch((err) => {
        console.error('[embedpdf] stamp library import failed:', err);
        setError(t('demo.stampsImportError'));
      })
      .finally(() => setBusy(null));
  };

  /** The library as the PDF it is — title, registry, artwork — for Acrobat
   *  or another viewer. */
  const exportPdf = (libraryId: string, name: string) => {
    const bytes = stamp.exportLibrary(libraryId);
    if (!bytes) return;
    const blob = new Blob([bytes as BlobPart], { type: 'application/pdf' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${name || 'stamps'}.pdf`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const shown = libraries
    .map((library) => ({
      library,
      items: assets.filter((asset) => asset.libraryId === library.id),
    }))
    .filter(({ items }) => items.length > 0);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-y-auto p-2">
        {busy === 'seeding' && shown.length === 0 ? (
          <div className="flex flex-col items-center gap-3 px-4 py-10 text-center">
            <div className="border-border-subtle border-t-accent h-6 w-6 animate-spin rounded-full border-2" />
            <p className="text-fg-muted text-sm">{t('demo.stampsLoading')}</p>
          </div>
        ) : shown.length === 0 ? (
          <div className="flex flex-col items-center gap-3 px-4 py-10 text-center">
            <Icon name="rubberStamp" size={32} className="text-fg-muted" />
            <p className="text-fg-muted text-sm">{t('demo.stampsEmpty')}</p>
          </div>
        ) : (
          shown.map(({ library, items }) => (
            <section key={library.id} className="mb-3 last:mb-0">
              <header className="flex items-center gap-1 px-1 py-1">
                <h3 className="text-fg-muted min-w-0 flex-1 truncate text-xs font-semibold uppercase tracking-wide">
                  {library.name}
                </h3>
                <button
                  type="button"
                  onClick={() => exportPdf(library.id, library.name)}
                  title={t('demo.stampsExportLibrary')}
                  className="text-fg-muted hover:text-fg grid h-6 w-6 shrink-0 place-items-center rounded"
                >
                  <Icon name="download" size={14} />
                </button>
                <button
                  type="button"
                  onClick={() => void stamp.removeLibrary(library.id)}
                  title={t('demo.stampsRemoveLibrary')}
                  className="text-fg-muted hover:text-fg grid h-6 w-6 shrink-0 place-items-center rounded"
                >
                  <Icon name="trash" size={14} />
                </button>
              </header>
              <ul className="flex flex-col gap-1">
                {items.map((asset) => (
                  <li key={asset.id}>
                    <StampAssetButton
                      asset={asset}
                      armed={armedId === asset.id}
                      onArm={() => arm(asset)}
                    />
                  </li>
                ))}
              </ul>
            </section>
          ))
        )}
      </div>

      {(error || armedId) && (
        <div
          className={`border-border-subtle shrink-0 border-t px-3 py-2 text-xs ${
            error ? 'text-red-600' : 'text-fg-muted'
          }`}
        >
          {error ?? t('demo.stampsArmedHint')}
        </div>
      )}

      <div className="border-border-subtle shrink-0 border-t p-3">
        <button
          type="button"
          disabled={busy != null}
          onClick={() => fileRef.current?.click()}
          title={t('demo.stampsImportHint')}
          className="border-border text-fg hover:bg-hover flex w-full items-center justify-center gap-2 rounded-md border px-3 py-1.5 text-sm disabled:opacity-40"
        >
          <Icon name="rubberStampPlus" size={16} />
          {busy === 'importing' ? t('demo.stampsImporting') : t('demo.stampsImport')}
        </button>
        <input
          ref={fileRef}
          type="file"
          accept="application/pdf"
          className="hidden"
          onChange={(e) => {
            const file = e.currentTarget.files?.[0];
            e.currentTarget.value = ''; // allow re-picking the same file
            if (file) importPdf(file);
          }}
        />
      </div>
    </div>
  );
}

/** One gallery row: the plugin's cached preview through the object-URL hook
 *  (the DOM resource's lifetime belongs to the framework layer, not the store). */
function StampAssetButton({
  asset,
  armed,
  onArm,
}: {
  asset: StampAsset;
  armed: boolean;
  onArm: () => void;
}) {
  const url = useStampAssetPreviewUrl(asset.id);
  return (
    <button
      type="button"
      onClick={onArm}
      // The label is what the stamp reads; the identifier (its /Name) is the
      // tooltip's second line for the curious.
      title={asset.label === asset.name ? asset.label : `${asset.label}\n${asset.name}`}
      aria-pressed={armed}
      className={`flex w-full items-center justify-center rounded-md border px-2 py-1.5 ${
        armed
          ? 'border-accent bg-accent-light'
          : 'border-border-subtle hover:border-border hover:bg-hover'
      }`}
    >
      {url ? (
        // Height-capped, width-bounded: a wide rubber stamp stays a compact
        // strip while a portrait page from an imported library still gets
        // enough pixels to be recognisable.
        <img src={url} alt={asset.label} className="max-h-12 max-w-full" />
      ) : (
        <span className="text-fg truncate text-sm">{asset.label}</span>
      )}
    </button>
  );
}
