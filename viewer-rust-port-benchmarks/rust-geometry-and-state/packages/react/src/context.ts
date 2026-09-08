import { createContext, useContext } from 'react';
import type { CoreStore } from '@poc/core';

export const CoreContext = createContext<CoreStore | null>(null);

export function useCore(): CoreStore {
  const store = useContext(CoreContext);
  if (!store) throw new Error('useCore must be used inside <CoreContext.Provider>');
  return store;
}
