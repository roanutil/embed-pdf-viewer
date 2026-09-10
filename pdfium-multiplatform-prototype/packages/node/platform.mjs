// Shared by the addon loader and build script so glibc/musl cannot diverge.
export function resolveTarget({ platform, arch, report } = process) {
  if (!['arm64', 'x64'].includes(arch)) return null;
  if (platform === 'darwin' || platform === 'win32') return `${platform}-${arch}`;
  if (platform !== 'linux') return null;
  const header = report?.getReport?.()?.header;
  // An unavailable report is indeterminate; preserve the glibc fallback.
  const musl = header != null && header.glibcVersionRuntime == null;
  return `${musl ? 'linuxmusl' : 'linux'}-${arch}`;
}
