using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
namespace EmbedPDF.Engine;

[StructLayout(LayoutKind.Sequential)]
internal struct NativeReply { internal ulong Result; internal ulong Document; }
internal static unsafe partial class NativeMethods
{
    static NativeMethods()
    {
        if (!OperatingSystem.IsWindows()) return;
        // Absolute dependency loading avoids changing the process DLL search path.
        var directory = AppContext.BaseDirectory;
        var dependency = Path.Combine(directory, "embedpdf.dll");
        if (!File.Exists(dependency))
            dependency = Path.Combine(directory, "runtimes", "win-x64", "native", "embedpdf.dll");
        NativeLibrary.Load(Path.GetFullPath(dependency), typeof(NativeMethods).Assembly,
            DllImportSearchPath.UseDllDirectoryForDependencies | DllImportSearchPath.System32);
    }
    const string Library = "epdf_capi";
    [LibraryImport(Library, EntryPoint = "epdf_abi_version")][UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])] internal static partial uint AbiVersion();
    [LibraryImport(Library, EntryPoint = "epdf_engine_create")][UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])] internal static partial uint Create(byte* options, ulong optionsLength, byte* payloads, ulong payloadsLength, out ulong engine, out ulong error);
    [LibraryImport(Library, EntryPoint = "epdf_engine_submit")][UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])] internal static partial uint Submit(ulong engine, ulong document, uint operation, byte* args, ulong argsLength, byte* payloads, ulong payloadsLength, out ulong job, out ulong error);
    [LibraryImport(Library, EntryPoint = "epdf_job_wait")][UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])] internal static partial uint Wait(ulong job, uint timeout, out NativeReply reply);
    [LibraryImport(Library, EntryPoint = "epdf_job_cancel")][UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])] internal static partial uint Cancel(ulong job);
    [LibraryImport(Library, EntryPoint = "epdf_job_release")][UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])] internal static partial uint ReleaseJob(ulong job);
    [LibraryImport(Library, EntryPoint = "epdf_engine_close")][UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])] internal static partial uint Close(ulong engine, uint timeout);
    [LibraryImport(Library, EntryPoint = "epdf_engine_release")][UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])] internal static partial uint ReleaseEngine(ulong engine);
    [LibraryImport(Library, EntryPoint = "epdf_helper_call")][UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])] internal static partial uint Helper(uint operation, byte* args, ulong argsLength, out ulong result);
    [LibraryImport(Library, EntryPoint = "epdf_result_view")][UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])] internal static partial uint View(ulong result, out byte* bytes, out ulong length);
    [LibraryImport(Library, EntryPoint = "epdf_result_release")][UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])] internal static partial uint ReleaseResult(ulong result);
}
