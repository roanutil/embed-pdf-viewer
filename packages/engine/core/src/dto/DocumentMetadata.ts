/**
 * `/Trapped`, generated from the Rust definition in
 * `crates/epdf-engine/src/dto.rs`.
 */
import type { DocumentMetadataTrapped } from './generated/DocumentMetadataTrapped';
import type { IsoDateTime } from './IsoDateTime';

export type { DocumentMetadataTrapped };

/**
 * The standard fields of the document's Info dict. The dict's other keys
 * are {@link CustomMetadata}, read and changed through `doc.metadata.custom`.
 */
export interface DocumentMetadata {
  title: string | null;
  author: string | null;
  subject: string | null;
  keywords: string | null;
  producer: string | null;
  creator: string | null;
  /** `/CreationDate`. */
  createdAt: IsoDateTime | null;
  /** `/ModDate`. */
  modifiedAt: IsoDateTime | null;
  trapped: DocumentMetadataTrapped;
}
