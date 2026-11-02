using System.Runtime.InteropServices;
namespace EmbedPDF.Engine;

internal abstract class NativeHandle : SafeHandle
{
    protected NativeHandle(ulong value) : base(0, true) { if (IntPtr.Size != 8) throw new PlatformNotSupportedException("EmbedPDF requires a 64-bit process"); SetHandle(unchecked((nint)value)); }
    public override bool IsInvalid => handle == 0;
    internal Lease Acquire() => new(this);
    internal sealed class Lease : IDisposable
    {
        NativeHandle? owner;
        internal ulong Value { get; }
        internal Lease(NativeHandle handle) { bool added = false; handle.DangerousAddRef(ref added); owner = handle; Value = unchecked((ulong)handle.DangerousGetHandle()); }
        public void Dispose() => Interlocked.Exchange(ref owner, null)?.DangerousRelease();
    }
}
internal sealed class EngineHandle(ulong value) : NativeHandle(value)
{
    protected override bool ReleaseHandle() { NativeMethods.ReleaseEngine(unchecked((ulong)handle)); return true; }
}
internal sealed class JobHandle(ulong value) : NativeHandle(value)
{
    internal void Cancel() { using var lease = Acquire(); NativeMethods.Cancel(lease.Value); }
    protected override bool ReleaseHandle() { NativeMethods.ReleaseJob(unchecked((ulong)handle)); return true; }
}
internal sealed class ResultHandle(ulong value) : NativeHandle(value)
{
    protected override bool ReleaseHandle() { NativeMethods.ReleaseResult(unchecked((ulong)handle)); return true; }
}
