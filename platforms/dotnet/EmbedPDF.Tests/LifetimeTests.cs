using System.Runtime.CompilerServices;
using EmbedPDF.Engine;
using Xunit;
namespace EmbedPDF.Tests;

public class LifetimeTests
{
    [Fact]
    public async Task DocumentCloseRemainsAvailableWhenManagedSlotsAreFull()
    {
        await using var engine = await PdfEngine.CreateAsync(new() { MaxPendingRequests = 1 });
        for (int i = 0; i < 20; i++)
        {
            var doc = await engine.OpenAsync(FacadeTests.Pdf);
            var read = doc.GetPagesAsync(); var close = doc.DisposeAsync().AsTask();
            await Task.WhenAll(read, close);
            await Assert.ThrowsAsync<ObjectDisposedException>(() => doc.GetPagesAsync());
        }
    }
    [MethodImpl(MethodImplOptions.NoInlining)]
    static async Task<WeakReference> AbandonEngine()
    {
        var engine = await PdfEngine.CreateAsync(); return new WeakReference(engine);
    }
    [Fact]
    public async Task AbandonedEngineCanBeFinalizedWithoutAnExplicitDispose()
    {
        var reference = await AbandonEngine();
        for (int i = 0; i < 20 && reference.IsAlive; i++) { GC.Collect(); GC.WaitForPendingFinalizers(); await Task.Delay(20); }
        Assert.False(reference.IsAlive);
    }
    [Fact]
    public async Task PasswordErrorsAreTypedAndDoNotRetainParkedBytes()
    {
        var bytes = File.ReadAllBytes(Path.Combine(FacadeTests.Root, "tooling/engine-conformance/fixtures/encrypted-r4-aes-128.pdf"));
        await using var engine = await PdfEngine.CreateAsync(new() { Limits = new() { MaxRetainedInputBytes = (ulong)bytes.Length + 16 } });
        for (int i = 0; i < 3; i++)
        {
            Assert.Equal("DocPasswordRequired", (await Assert.ThrowsAsync<PdfException>(() => engine.OpenAsync(bytes))).Code);
            Assert.Equal("DocPasswordIncorrect", (await Assert.ThrowsAsync<PdfException>(() => engine.OpenAsync(bytes, "wrong"))).Code);
        }
        await using var doc = await engine.OpenAsync(bytes, "user"); Assert.NotEmpty(await doc.GetPagesAsync());
    }
    [Fact]
    public async Task NativeCreateRefusalMapsStatusDeterministically()
    {
        var exception = await Assert.ThrowsAsync<PdfException>(() => PdfEngine.CreateAsync(new() { Limits = new() { MaxPendingJobs = 0 } }));
        Assert.Equal("InvalidArg", exception.Code); Assert.Equal(2u, exception.NativeStatus);
    }
    [Fact]
    public async Task NativeResultAndJobSlotsRemainUsableAfterCanceledWork()
    {
        await using var engine = await PdfEngine.CreateAsync(new() { MaxPendingRequests = 2, Limits = new() { MaxPendingJobs = 1, MaxRetainedInputBytes = (ulong)FacadeTests.Pdf.Length * 2 } });
        for (int pass = 0; pass < 15; pass++)
        {
            using var cancellation = new CancellationTokenSource();
            var opening = engine.OpenAsync(FacadeTests.Pdf, cancellationToken: cancellation.Token); cancellation.Cancel();
            try { await using var canceled = await opening; } catch (OperationCanceledException) { }
            await using var document = await engine.OpenAsync(FacadeTests.Pdf);
            Assert.NotEmpty(await document.GetPagesAsync());
        }
    }
}
