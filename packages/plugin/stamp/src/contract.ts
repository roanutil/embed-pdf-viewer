/** Stamp-library capability protocol without import/render/plugin wiring. */
export { StampToken } from './types';
export type {
  AddAssetInput,
  ImportLibraryOptions,
  StampAsset,
  StampAssetKind,
  StampAssetPreview,
  StampCapability,
  StampConfig,
  StampLibrary,
  StampLibraryChange,
  StampState,
} from './types';
export { parseStampKey, stampKey, customStampName, assetIdFor } from './convention';
export type { StampKey } from './convention';
export type { StampLibraryStore } from './persistence';
