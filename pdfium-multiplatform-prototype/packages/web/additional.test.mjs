import { additionalVectors } from '../test-support/additional-vectors.mjs';
import { createEpdf } from './index.mjs';
const epdf = await createEpdf();
await additionalVectors(epdf.openDocument);
