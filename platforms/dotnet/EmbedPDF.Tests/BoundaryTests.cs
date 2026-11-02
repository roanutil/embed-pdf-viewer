using System.Collections.Immutable;
using System.Text.Json;
using EmbedPDF.Engine;
using Xunit;
namespace EmbedPDF.Tests;

public class BoundaryTests
{
    [Fact]
    public void RequiredGeometryFieldsCannotDecodeAsZeroGeometry()
    {
        foreach (var json in new[] { "{}", "{\"runs\":[{\"start\":0,\"glyphs\":[]}]}", "{\"runs\":[{\"rect\":{\"x\":0,\"y\":0,\"width\":1,\"height\":1},\"start\":0,\"glyphs\":[],\"rotation\":0}]}" })
            Assert.Throws<JsonException>(() => Wire.Read<PageGeometrySnapshot>(JsonDocument.Parse(json).RootElement));
    }
    [Fact]
    public async Task InvalidBackgroundDoesNotSilentlyRenderWhite()
    {
        await using var engine = await PdfEngine.CreateAsync(); await using var doc = await engine.OpenAsync(FacadeTests.Pdf);
        await Assert.ThrowsAsync<ArgumentOutOfRangeException>(() => doc.RenderAsync(new(0), new() { Background = (RenderBackground)99, IncludeAnnotations = false, IncludeFormFields = false }));
    }
    [Fact]
    public async Task BinaryStartupFontsRegisterBeforeOpenAndInvalidFontsRefuseCreation()
    {
        var file = Path.Combine(FacadeTests.Root, "packages/engine/main/test/fixtures/Roboto-Regular.ttf");
        var font = new StartupFont("test-first", File.ReadAllBytes(file)) { FamilyName = "test", Fallback = true };
        await using (var engine = await PdfEngine.CreateAsync(new() { StartupFonts = [font] })) { await using var doc = await engine.OpenAsync(FacadeTests.Pdf); Assert.NotEmpty((await doc.GetTextAsync(new(0))).Text); }
        await Assert.ThrowsAsync<PdfException>(() => PdfEngine.CreateAsync(new() { StartupFonts = [new("invalid", new byte[] { 0, 1, 2 })] }));
    }
    [Fact]
    public async Task ByteAdmissionRejectsBeforeCopyAndRecoversToBaseline()
    {
        await using var engine = await PdfEngine.CreateAsync(new() { MaxQueuedBytes = 128 });
        var error = await Assert.ThrowsAsync<PdfException>(() => engine.OpenAsync(FacadeTests.Pdf)); Assert.Equal("Busy", error.Code); Assert.Equal(0, engine.PendingRequests); Assert.Equal(0, engine.QueuedBytes);
    }
    [Fact]
    public void ErrorFramesMapCancellationAndUnknownStatusWithoutLastError()
    {
        Assert.IsType<OperationCanceledException>(Record.Exception(() => Wire.Decode(BuildError("Aborted"))));
        var first = Assert.Throws<PdfException>(() => Wire.Status(8)); var second = Assert.Throws<PdfException>(() => Wire.Status(99)); Assert.Equal("RuntimeUnavailable", first.Code); Assert.Equal("UnknownStatus", second.Code);
    }
    [Fact]
    public async Task ResultViewLeaseSurvivesConcurrentManagedDispose()
    {
        var json = Wire.Encode(new { text = new { text = "abc", charCount = 3 }, boundary = 1 }); ulong id; uint status;
        unsafe { fixed (byte* args = json) status = NativeMethods.Helper(5, args, (ulong)json.Length, out id); }
        Assert.Equal(0u, status); using var result = new ResultHandle(id);
        using (var lease = result.Acquire())
        {
            await Task.Run(result.Dispose);
            unsafe
            {
                Assert.Equal(0u, NativeMethods.View(lease.Value, out var bytes, out var length));
                Assert.Equal(1, Wire.Decode(new ReadOnlySpan<byte>(bytes, checked((int)length))).Result.GetInt32());
            }
        }
        unsafe { Assert.Equal(3u, NativeMethods.View(id, out _, out _)); }
    }
    [Fact]
    public void MalformedSnapshotsAndSingularTransformsRefuseClearly()
    {
        var text = new PageTextSnapshot { Text = "ab", CharCount = 1 };
        Assert.Equal("InvalidArg", Assert.Throws<PdfException>(() => TextGeometry.GetTextOffset(text, new(0))).Code);
        Assert.Equal("InvalidArg", Assert.Throws<PdfException>(() => PageTransforms.Apply(new(new(100, 200), PageRotation.None, 1e-200, 1e200))).Code);
    }
    [Fact]
    public void TransformMetricsIncludeNativeCssMatrix()
    {
        var transform = PageTransforms.Apply(new(new(100, 200), PageRotation.Clockwise90, 1.25, 2));
        Assert.NotEmpty(transform.Transform.CssMatrix);
    }
    static byte[] BuildError(string code)
    {
        var json = Wire.Encode(new { error = new { code, message = "test" } }); var frame = new byte[json.Length + 8]; System.Buffers.Binary.BinaryPrimitives.WriteUInt32LittleEndian(frame, (uint)json.Length); json.CopyTo(frame, 4); return frame;
    }
}
