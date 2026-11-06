/** Rejects oracle runs that lose the attachment corpus's intended success/refusal cases. */
export function assertAttachmentCoverage(
  downloads: readonly { bytes?: string; errorCode?: string }[],
): void {
  if (!downloads.some((item) => item.bytes !== undefined)) {
    throw new Error(
      'Attachment corpus lost all decoded files; fix the oracle/runtime before regenerating.',
    );
  }
  if (!downloads.some((item) => item.errorCode === 'InvalidArg')) {
    throw new Error('Attachment corpus lost its missing-stream refusal coverage.');
  }
}
