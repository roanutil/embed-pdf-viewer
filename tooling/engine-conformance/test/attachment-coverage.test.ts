import assert from 'node:assert/strict';
import test from 'node:test';
import { assertAttachmentCoverage } from '../src/attachment-coverage';

test('zero-byte downloads count as successful decoding beside a real refusal', () => {
  assert.doesNotThrow(() =>
    assertAttachmentCoverage([{ bytes: '' }, { bytes: '00' }, { errorCode: 'InvalidArg' }]),
  );
});

test('an all-error oracle cannot replace successful attachment fixtures', () => {
  assert.throws(() =>
    assertAttachmentCoverage([{ errorCode: 'Unknown' }, { errorCode: 'InvalidArg' }]),
  );
});

test('successful downloads cannot erase missing-stream refusal coverage', () => {
  assert.throws(() => assertAttachmentCoverage([{ bytes: '00' }]));
});
