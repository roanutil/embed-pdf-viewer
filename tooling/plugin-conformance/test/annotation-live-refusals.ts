/** Exact named intentional refusals and malformed-protocol negatives. Successful behavior is asserted independently. */
export const expectedRefusals: Readonly<
  Record<string, readonly [number, string, readonly string[]][]>
> = {
  'create-from-page-space': [
    [19, 'settled', ['full']],
    [19, 'callbacks', ['full']],
    [20, 'settled', ['page 3 is not in this document']],
    [21, 'settled', ['intended invalid geometry']],
    [21, 'callbacks', ['intended invalid geometry']],
    [22, 'settled', ["no tool 'nope'"]],
    [23, 'settled', ["a kind the engine doesn't know can't be created"]],
  ],
  'create-without-authority': [
    [3, 'settled', ['annotation.create requires annotations:create']],
    [4, 'settled', ['annotation.createFromSelection requires annotations:create']],
    [5, 'settled', ['annotation.create requires annotations:create']],
  ],
  'update-in-page-space': [
    [18, 'settled', ['intended invalid geometry']],
    [18, 'callbacks', ['intended invalid geometry']],
    [19, 'settled', ['no annotation obj:99']],
    [22, 'settled', ['intended locked delete']],
    [22, 'callbacks', ['intended locked delete']],
  ],
  'selection-verbs': [[13, 'settled', ['failed:1', 'intended selection refusal']]],
  'set-contents-settles-with-the-engine': [
    [2, 'settled', ['read-only']],
    [3, 'settled', ['read-only']],
  ],
  'attachments-stamps-and-downloads': [
    [11, 'settled', ['page 7 is not in this document']],
    [12, 'settled', ['too big']],
    [12, 'callbacks', ['too big']],
    [14, 'settled', ['a stamp must be PNG, JPEG, or one-page PDF bytes']],
    [15, 'settled', ['page 7 is not in this document']],
  ],
  'remeasure-page': [[2, 'settled', ['failed:1', 'intended stale measurement']]],
  'remeasure-without-authority-and-after-errors': [
    [4, 'settled', ['the annotations of this document are not loaded']],
  ],
  'v3-document-events': [[5, 'reports', ['intended busy']]],
  'awaited-writes-and-pending-text': [[3, 'settled', ['intended locked rotation']]],
  'stamp-preview-providers': [[5, 'callbacks', ['intended raster failure']]],
  'create-programmatically': [
    [4, 'settled', ['nope']],
    [5, 'settled', ['page 99 is not in this document']],
  ],
  'update-and-delete-by-ref': [
    [4, 'settled', ['stale']],
    [5, 'settled', ['no annotation obj:42']],
    [6, 'settled', ['no annotation base:1:0']],
    [8, 'settled', ['locked']],
    [9, 'settled', ['no annotation obj:42']],
  ],
  'restyle-writes-and-rolls-back': [
    [5, 'settled', ['failed:2', 'intended restyle refusal', 'intended restyle refusal']],
  ],
  'flags-writes': [[5, 'settled', ['failed:2', 'denied', 'denied']]],
  'delete-and-rotate-the-selection': [
    [7, 'settled', ['failed:2', 'intended delete refusal', 'intended delete refusal']],
  ],
  'records-without-a-ref': [
    [2, 'reports', ["Cannot read properties of undefined (reading 'kind')"]],
    [5, 'settled', ["Cannot read properties of undefined (reading 'kind')"]],
  ],
  'restored-page-reloads': [[4, 'reports', ['intended reload failure']]],
  'reconcile/create-failed': [[5, 'settled', ['intended create refusal']]],
  'reconcile/create-failed-mid-move': [
    [8, 'settled', ['intended pending-create refusal']],
    [8, 'callbacks', ['intended pending-create refusal']],
  ],
  'reconcile/create-failed-mid-handle': [
    [8, 'settled', ['intended pending-create refusal']],
    [8, 'callbacks', ['intended pending-create refusal']],
  ],
  'reconcile/h3-pending-stamp-refuse': [
    [8, 'settled', ['intended stamp refusal']],
    [8, 'callbacks', ['intended stamp refusal']],
  ],
};
