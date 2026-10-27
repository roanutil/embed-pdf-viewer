/** Exact intentional engine, permission and malformed-binary-source failures. */
export const expectedRefusals: Readonly<
  Record<string, readonly [number, string, readonly string[]][]>
> = {
  'reply-edit-status-marked': [
    [5, 'settled', ['no annotation obj:99']],
    [6, 'settled', ["a measurement's text is worked out from its points"]],
    [11, 'settled', ['[annotation] no comment thread contains this ref']],
  ],
  'remove-and-remove-thread-atomically': [
    [4, 'settled', ['failed:1', 'delete not permitted']],
    [5, 'settled', ['failed:2', 'delete not permitted', 'delete not permitted']],
    [7, 'settled', ['failed:1', 'intended atomic thread refusal']],
    [8, 'settled', ['[annotation] no comment thread contains this ref']],
  ],
  'attachment-download': [[3, 'settled', ['no annotation obj:7']]],
  'group-refused-per-record': [
    [5, 'settled', ['intended denied group']],
    [6, 'settled', ['intended denied ungroup']],
    [11, 'settled', ['intended group refusal']],
  ],
  'malformed-stamp-source-inputs': [
    [2, 'settled', ["Cannot read properties of undefined (reading 'arrayBuffer')"]],
    [3, 'settled', ["Cannot destructure property 'data' of 'source' as it is null."]],
    [4, 'settled', ['data.arrayBuffer is not a function']],
    [5, 'settled', ["Cannot read properties of undefined (reading 'arrayBuffer')"]],
    [6, 'settled', ["Cannot destructure property 'data' of 'source' as it is undefined."]],
    [7, 'settled', ["Cannot read properties of undefined (reading 'arrayBuffer')"]],
  ],
  'a-bad-config-tool-fails-the-capability': [
    [1, 'settled', ["[annotation] tool 'square' does not support default 'fontSize' for 'square'"]],
    [2, 'outcome', ['Capability "annotation" unavailable: document "a" is error.']],
  ],
  'note-placement-and-ghost': [[14, 'callbacks', ['intended note refusal']]],
  'stamp-prompt-through-the-file-picker': [
    [10, 'callbacks', ['dialog broke']],
    [
      17,
      'callbacks',
      ['console.error:["[annotation]","a stamp must be PNG, JPEG, or one-page PDF bytes"]'],
    ],
  ],
  'attachment-placement': [[7, 'callbacks', ['intended attachment refusal']]],
  'armed-stamps': [[26, 'settled', ['a stamp must be PNG, JPEG, or one-page PDF bytes']]],
  'fixed-bytes-stamp-tool': [
    [
      7,
      'callbacks',
      ['console.error:["[annotation]","a stamp must be PNG, JPEG, or one-page PDF bytes"]'],
    ],
    [20, 'outcome', ["[annotation] tool 'bad' does not support default 'fontSize' for 'square'"]],
  ],
};
