using System.Diagnostics;
using EmbedPDF.Engine;
using Xunit;
namespace EmbedPDF.Tests;

public class RenderTests
{
    [Theory]
    [InlineData(PageRotation.None)]
    [InlineData(PageRotation.Clockwise90)]
    [InlineData(PageRotation.Clockwise180)]
    [InlineData(PageRotation.Clockwise270)]
    public async Task FullAndRegionRendersKeepActualDimensionsAndOwnedRgba(PageRotation rotation)
    {
        await using var engine = await PdfEngine.CreateAsync(); await using var doc = await engine.OpenAsync(FacadeTests.Pdf);
        foreach (var region in new PageRect?[] { null, new(2.25, 3.5, 20, 10) })
        {
            var image = await doc.RenderAsync(new(0), new() { Region = region, Rotation = rotation, Scale = 1.5, IncludeAnnotations = false, IncludeFormFields = false, Background = RenderBackground.Transparent });
            Assert.Equal(image.Stride * image.Height, image.Bytes.Length); if (region is { } area) { Assert.Equal(area, image.Area); Assert.Equal(rotation is PageRotation.Clockwise90 or PageRotation.Clockwise270 ? 15 : 30, image.Width); }
            Assert.Contains(image.Bytes.ToArray().Chunk(4), pixel => pixel[3] == 0);
        }
    }
    [Fact]
    public async Task OversizedAndNonfiniteRenderingIsRefused()
    {
        await using var engine = await PdfEngine.CreateAsync(); await using var doc = await engine.OpenAsync(FacadeTests.Pdf);
        await Assert.ThrowsAsync<PdfException>(() => doc.RenderAsync(new(0), new() { MaxOutputPixels = 1, IncludeAnnotations = false, IncludeFormFields = false }));
        await Assert.ThrowsAsync<ArgumentException>(() => doc.RenderAsync(new(0), new() { Scale = double.NaN }));
    }
    [Fact]
    [Trait("NativeRuntime", "DefaultLayers")]
    public async Task DefaultAnnotationAndWidgetRenderingWorks()
    {
        const string flag = "EPDF_DEFAULT_RENDER_PROBE";
        if (Environment.GetEnvironmentVariable(flag) == "1")
        {
            await using var engine = await PdfEngine.CreateAsync(); await using var doc = await engine.OpenAsync(File.ReadAllBytes(Path.Combine(FacadeTests.Root, "tooling/engine-conformance/fixtures/annotation_kinds.pdf")));
            var image = await doc.RenderAsync(new(0), new()); Assert.True(image.Bytes.Length > 0); return;
        }
        var start = new ProcessStartInfo("dotnet") { WorkingDirectory = FacadeTests.Root, RedirectStandardOutput = true, RedirectStandardError = true };
        foreach (var arg in new[] { "test", "platforms/dotnet/EmbedPDF.Tests/EmbedPDF.Tests.csproj", "--no-build", "--no-restore", "--filter", "FullyQualifiedName=EmbedPDF.Tests.RenderTests.DefaultAnnotationAndWidgetRenderingWorks" }) start.ArgumentList.Add(arg);
        start.Environment[flag] = "1"; start.Environment["COMPlus_DbgEnableMiniDump"] = "0";
        using var process = Process.Start(start)!; var stdout = process.StandardOutput.ReadToEndAsync(); var stderr = process.StandardError.ReadToEndAsync();
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(30));
        try { await process.WaitForExitAsync(deadline.Token); } catch (OperationCanceledException) { process.Kill(entireProcessTree: true); throw; }
        var output = await stdout + await stderr;
        Assert.True(process.ExitCode == 0, $"Default layer rendering failed in native subprocess (exit {process.ExitCode}).\n{output}");
    }
}
