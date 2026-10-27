// Named refusals reviewed against case intent and current upstream.
// Paged-empty refusal independently reproduced; see task-12b2a report/controller ruling.
export const expectedRefusals: Record<string, readonly (readonly [number, string, string])[]> = {
  'denied-in-rects-is-an-error': [
    [2, 'permission-denied', 'permission denied (local): doc.text.search'],
  ],
  'denied-mid-chain-is-an-error': [
    [2, 'permission-denied', 'permission denied (local): doc.text.copy'],
  ],
  'a-second-stale-cursor-is-an-error': [[2, 'invalid-input', 'stale cursor']],
  'invalid-query-is-an-error': [[2, 'invalid-input', 'invalid regex']],
  'engine-error-after-hits': [[3, 'operation-failed', 'boom']],
  'non-engine-errors-report-their-message': [[2, 'operation-failed', 'worker gone']],
  rerun: [[4, 'operation-failed', 'boom']],
  'verbs-carry-no-gate': [[3, 'permission-denied', 'search requires doc.text.search']],
  'session-events': [
    [17, 'operation-failed', 'boom'],
    [20, 'operation-failed', 'boom'],
  ],
  'close-races-the-session-and-find-all': [
    [4, 'instance-closed', 'document instance a#1 was closed'],
  ],
  'close-races-find-all-first': [[4, 'instance-closed', 'document instance a#1 was closed']],
  'find-all-pinned-mode': [[3, 'permission-denied', 'permission denied (local): doc.text.copy']],
  'find-all-errors-reject': [[2, 'operation-failed', 'boom']],
  'find-all-signal-aborts-the-slice': [
    [3, 'operation-cancelled', 'findAll aborted'],
    [5, 'operation-cancelled', 'findAll aborted'],
  ],
  'find-all-signal-already-aborted': [[3, 'operation-cancelled', 'findAll aborted']],
  'find-all-signal-on-an-error': [[2, 'operation-failed', 'boom']],
  'requires-interaction': [
    [
      0,
      'Error',
      'Plugin "selection" requires capability "interaction", which no plugin provides \u2014 add interactionPlugin() from \'@embedpdf/plugin-interaction\' to your plugins list.',
    ],
  ],
  'programmatic-select': [
    [18, 'not-found', 'page 77 is not in this document'],
    [19, 'not-found', 'page 76 is not in this document'],
    [20, 'not-found', 'page 78 is not in this document'],
  ],
  'select-refused': [
    [2, 'permission-denied', 'selection.select requires doc.text.select'],
    [3, 'permission-denied', 'selection.selectAll requires doc.text.select'],
  ],
  'read-text': [
    [5, 'operation-failed', 'gone'],
    [12, 'permission-denied', 'selection.readText requires doc.text.copy'],
  ],
  'select-page': [[11, 'not-found', 'page 42 is not in this document']],
  'refused-writes-and-copy-only': [
    [2, 'permission-denied', 'selection.selectPage requires doc.text.select'],
    [3, 'permission-denied', 'selection.selectWordAt requires doc.text.select'],
    [4, 'permission-denied', 'selection.selectLineAt requires doc.text.select'],
    [5, 'permission-denied', 'selection.extendTo requires doc.text.select'],
  ],
  'read-text-in-range': [
    [10, 'not-found', 'page 42 is not in this document'],
    [11, 'not-found', 'page 43 is not in this document'],
    [15, 'permission-denied', 'selection.readTextInRange requires doc.text.copy'],
  ],
  'read-text-cancelled': [
    [4, 'operation-cancelled', 'reading the text was cancelled'],
    [6, 'operation-cancelled', 'operation cancelled'],
    [13, 'operation-cancelled', 'operation cancelled'],
  ],
  'a-document-with-no-pages': [
    [36, 'TypeError', "Cannot read properties of undefined (reading 'x')"],
    [37, 'TypeError', "Cannot read properties of undefined (reading 'x')"],
    [38, 'TypeError', "Cannot read properties of undefined (reading 'x')"],
    [39, 'TypeError', "Cannot read properties of undefined (reading 'x')"],
    [40, 'TypeError', "Cannot read properties of undefined (reading 'x')"],
    [42, 'TypeError', "Cannot read properties of undefined (reading 'x')"],
    [43, 'TypeError', "Cannot read properties of undefined (reading 'x')"],
  ],
  'grid-nan-camera': [
    [3, 'TypeError', "Cannot read properties of undefined (reading 'pages')"],
    [6, 'TypeError', "Cannot read properties of undefined (reading 'pages')"],
    [7, 'TypeError', "Cannot read properties of undefined (reading 'pages')"],
  ],
  'grid-nan-zoom-factor': [
    [3, 'TypeError', "Cannot read properties of undefined (reading 'pages')"],
    [5, 'TypeError', "Cannot read properties of undefined (reading 'pages')"],
    [7, 'TypeError', "Cannot read properties of undefined (reading 'pages')"],
  ],
  'grid-fractional-columns': [
    [4, 'TypeError', "Cannot read properties of undefined (reading 'pages')"],
    [7, 'TypeError', "Cannot read properties of undefined (reading 'pages')"],
    [8, 'TypeError', "Cannot read properties of undefined (reading 'pages')"],
    [10, 'TypeError', "Cannot read properties of undefined (reading 'pages')"],
  ],
};
