// Carries a `?stamp-instance=N` query from an importing module to every
// module it imports under packages/plugin/stamp/src, so one import of the
// stamp controller with the query loads a whole private copy of the stamp
// source tree (and its module-level counter).
export async function resolve(specifier, context, next) {
  const resolved = await next(specifier, context);
  const parent = context.parentURL ?? '';
  const at = parent.indexOf('?stamp-instance=');
  if (at < 0 || resolved.url.includes('?') || !resolved.url.includes('/packages/plugin/stamp/src/')) return resolved;
  return { ...resolved, url: resolved.url + parent.slice(at) };
}
