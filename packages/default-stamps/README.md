# @embedpdf/default-stamps

The standard rubber-stamp library — Approved, Draft, Confidential, Sign Here, … —
as one PDF per locale, in Acrobat's own stamp-library dialect:

- `/Title` is the library name (`Standaard stempels`).
- `/Names /Pages` registers every page as `identifier=label`
  (`Approved=Goedgekeurd`): the identifier becomes a placed stamp's `/Name`,
  the label its `/Subj` and the picker text.
- Every page is one vector stamp; `/PieceInfo` carries the library id
  (`embedpdf-standard`), the locale, and each stamp's kind.

No manifest: the PDF carries everything. The same file drops into Acrobat's
`Stamps` folder and shows up as a library there.

| Locale  | File               | Name               |
| ------- | ------------------ | ------------------ |
| `en`    | `en/stamps.pdf`    | Standard Stamps    |
| `de`    | `de/stamps.pdf`    | Standardstempel    |
| `nl`    | `nl/stamps.pdf`    | Standaard stempels |
| `fr`    | `fr/stamps.pdf`    | Tampons standards  |
| `es`    | `es/stamps.pdf`    | Sellos estándar    |
| `zh-CN` | `zh-CN/stamps.pdf` | 标准印章           |
| `sv`    | `sv/stamps.pdf`    | Standardstämplar   |
| `ja`    | `ja/stamps.pdf`    | 標準スタンプ       |

Every locale registers the same 17 identifiers in the same page order; only
the labels differ. Import two locales side by side and the plugin keeps them
as two libraries with one identity.

## Use

```ts
import { StampToken } from '@embedpdf/plugin-stamp';

const stamp = registry.capability(StampToken);
const url = new URL('@embedpdf/default-stamps/nl/stamps.pdf', import.meta.url); // or your CDN
const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
await stamp.importLibraryPdf(bytes); // title, identifiers, labels come from the file
```

Fall back to `en` for a locale that is not shipped.

### Bundler-resolved URLs

`@embedpdf/default-stamps/urls` exports `urls[locale]` as static
`new URL(..., import.meta.url)` expressions, so webpack, Vite, Turbopack,
Rspack, and Parcel copy the PDFs into your build output and hand you their
URLs — no CDN, no manual asset copying — plus `LOCALES` and `CDN_URL_TEMPLATE`
(a jsDelivr fallback pinned to this major) for toolchains that flatten
`import.meta.url`. `@embedpdf/viewer-chrome` loads its built-in library this
way.

## Provenance

The artwork is Acrobat's standard stamp set, carried over unchanged from the
previous major of this package; only the catalog was added (title, registry,
PieceInfo). The stamp plugin's test suite re-imports every shipped file and
checks its title, identifiers, and labels.
