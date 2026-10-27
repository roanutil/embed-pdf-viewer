/** Scope module resolution to a preload; ordinary stamp imports retain their shared identities. */
import { registerHooks } from 'node:module';

export async function withStampModuleIsolation<Value>(load: () => Promise<Value>): Promise<Value> {
  const hook = registerHooks({
    resolve(specifier, context, next) {
      const resolved = next(specifier, context);
      const parent = context.parentURL ?? '';
      const query = parent.indexOf('?stamp-instance=current-');
      if (
        query < 0 ||
        resolved.url.includes('?') ||
        !resolved.url.includes('/packages/plugin/stamp/src/')
      )
        return resolved;
      return { ...resolved, url: resolved.url + parent.slice(query) };
    },
  });
  try {
    return await load();
  } finally {
    hook.deregister();
  }
}
