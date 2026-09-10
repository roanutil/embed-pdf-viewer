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
import { loadDefaultLibrary } from '@embedpdf/default-stamps/library';

const stamp = registry.capability(StampToken);
await stamp.importLibraryPdf(await loadDefaultLibrary('nl')); // title, identifiers, labels come from the file
```

### As part of your build

`@embedpdf/default-stamps/library` delivers each locale through the module
graph — a generated ES module per locale, loaded with a literal dynamic
import — so the library ships as a lazy chunk of your own build. Nothing to
copy, no asset pipeline to configure, no CDN:

```ts
import { LOCALES, loadDefaultLibrary } from '@embedpdf/default-stamps/library';

const bytes = await loadDefaultLibrary('nl'); // the PDF; unknown codes fall back to `en`
await stamp.importLibraryPdf(bytes);
```

The PDFs themselves stay in the package (`<locale>/stamps.pdf`) for
self-hosting and for Acrobat.

## Provenance

The artwork is Acrobat's standard stamp set, carried over unchanged from the
previous major of this package; only the catalog was added (title, registry,
PieceInfo). The stamp plugin's test suite re-imports every shipped file and
checks its title, identifiers, and labels.
