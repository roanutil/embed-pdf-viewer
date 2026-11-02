using System.Reflection;
using System.Runtime.InteropServices;
using System.Runtime.CompilerServices;
using System.Text.Json;
using EmbedPDF.Engine;
using Xunit;
[assembly: CollectionBehavior(DisableTestParallelization = true)]
namespace EmbedPDF.Tests;

public class FacadeTests
{
    internal static string Root => FindRoot();
    static string FindRoot() { var p = new DirectoryInfo(AppContext.BaseDirectory); while (p != null && !Directory.Exists(Path.Combine(p.FullName, "crates"))) p = p.Parent; return p!.FullName; }
    [ModuleInitializer] internal static void LoadNative() => NativeLibrary.SetDllImportResolver(typeof(PdfEngine).Assembly, (name, assembly, path) => name == "epdf_capi" ? NativeLibrary.Load(Path.Combine(Environment.GetEnvironmentVariable("EPDF_NATIVE_DIR") ?? throw new InvalidOperationException("Set test-only EPDF_NATIVE_DIR to cargo build output"), OperatingSystem.IsWindows() ? "epdf_capi.dll" : OperatingSystem.IsMacOS() ? "libepdf_capi.dylib" : "libepdf_capi.so")) : 0);
    internal static byte[] Pdf => File.ReadAllBytes(Path.Combine(Root, "tooling/engine-conformance/fixtures/embedded_orientations.pdf"));
    [Fact]
    public async Task OpenReadAndHelpers()
    {
        await using var engine = await PdfEngine.CreateAsync();
        await using var doc = await engine.OpenAsync(Pdf);
        var pages = await doc.GetPagesAsync(); Assert.NotEmpty(pages); Assert.Equal(new PageIndex(0), pages[0].Index); Assert.True(pages[0].Size.Width > 0); Assert.True(pages[0].Reference.ObjectNumber > 0);
        var text = await doc.GetTextAsync(new(0)); Assert.NotEmpty(text.Text);
        var geometry = await doc.GetGeometryAsync(new(0)); Assert.NotEmpty(geometry.Runs);
        var segments = TextGeometry.GetSegments(geometry, new(new(0), 1)); Assert.NotEmpty(segments);
        Assert.NotNull(TextGeometry.GetCharacterQuad(geometry, new(0)));
        Assert.NotEmpty(TextGeometry.SliceText(text, new(new(0), 1)));
        Assert.Equal(new Utf16Offset(0), TextGeometry.GetTextOffset(text, new(0)));
        Assert.True(TextGeometry.GetCharacterRange(text, new(0), new(1)).Count > 0);
        var search = await doc.SearchLiteralAsync(new("UPRIGHT") { Snippets = true, MaxPages = 1 }); Assert.True(search.PagesSearched > 0);
        var transform = PageTransforms.Apply(new(new(100, 200), PageRotation.Clockwise90, 1.25, 2), pagePoints: [new(1, 2)]); Assert.Single(transform.PageToView);
    }
    [Fact]
    public async Task ImagesOwnBytesAfterClose()
    {
        var engine = await PdfEngine.CreateAsync(); var doc = await engine.OpenAsync(Pdf);
        var image = await doc.RenderAsync(new(0), new() { Region = new(0, 0, 20, 10), Scale = 2, IncludeAnnotations = false, IncludeFormFields = false, Background = RenderBackground.Transparent });
        await doc.DisposeAsync(); await engine.DisposeAsync(); Assert.Equal(40, image.Width); Assert.Equal(20, image.Height); Assert.Equal(image.Stride * image.Height, image.Bytes.Length);
    }
    [Fact]
    public async Task RepeatedConcurrentDisposeAndStaleChild()
    {
        var engine = await PdfEngine.CreateAsync(); var doc = await engine.OpenAsync(Pdf);
        await Task.WhenAll(Enumerable.Range(0, 8).Select(_ => doc.DisposeAsync().AsTask()));
        await Assert.ThrowsAsync<ObjectDisposedException>(() => doc.GetTextAsync(new(0)));
        var child = await engine.OpenAsync(Pdf); await Task.WhenAll(Enumerable.Range(0, 8).Select(_ => engine.DisposeAsync().AsTask()));
        await Assert.ThrowsAsync<ObjectDisposedException>(() => child.GetPagesAsync()); await child.DisposeAsync();
    }
    [Fact]
    public async Task ErrorsAndCancellation()
    {
        await using var engine = await PdfEngine.CreateAsync();
        var ex = await Assert.ThrowsAsync<PdfException>(() => engine.OpenAsync(new byte[] { 1, 2, 3 })); Assert.NotEmpty(ex.Code);
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => engine.OpenAsync(Pdf, cancellationToken: new(true)));
        await using var doc = await engine.OpenAsync(Pdf);
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => doc.GetGeometryAsync(new(0), new(true)));
    }
    [Fact]
    public async Task SlotAndByteBoundsTerminateAllWaiters()
    {
        var engine = await PdfEngine.CreateAsync(new() { MaxPendingRequests = 2, MaxQueuedBytes = Pdf.Length * 2 + 4096 });
        using var cancel = new CancellationTokenSource();
        var requests = Enumerable.Range(0, 60).Select(async i => { try { await using var d = await engine.OpenAsync(Pdf, cancellationToken: cancel.Token); await d.GetTextAsync(new(0), cancel.Token); } catch (Exception e) when (e is OperationCanceledException or ObjectDisposedException or PdfException) { } }).ToArray();
        cancel.Cancel(); await engine.DisposeAsync(); await Task.WhenAll(requests).WaitAsync(TimeSpan.FromSeconds(30));
        Assert.Equal(0, engine.PendingRequests); Assert.Equal(0, engine.QueuedBytes);
    }
}
