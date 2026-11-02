using System.Collections.Immutable;
using System.Text.Json;
namespace EmbedPDF.Engine;

public sealed class PdfDocument : IAsyncDisposable
{
    readonly object gate = new();
    readonly PdfEngine engine;
    readonly ulong document;
    Task? disposed;
    internal PdfDocument(PdfEngine engine, ulong document) { this.engine = engine; this.document = document; }
    Task<WireResult> Call(uint operation, object args, CancellationToken cancellation)
    {
        lock (gate)
        {
            if (disposed != null) return Task.FromException<WireResult>(new ObjectDisposedException(nameof(PdfDocument)));
            return engine.Enqueue(document, operation, args, null, cancellation);
        }
    }
    public async Task<ImmutableArray<PageInfo>> GetPagesAsync(CancellationToken cancellationToken = default)
        => Wire.Read<ImmutableArray<PageInfo>>((await Call(3, new { }, cancellationToken).ConfigureAwait(false)).Result.GetProperty("pages"));
    async Task<PageReference> Page(PageIndex index, CancellationToken cancellation)
    {
        var pages = await GetPagesAsync(cancellation).ConfigureAwait(false);
        if (index.Value >= (uint)pages.Length) throw new ArgumentOutOfRangeException(nameof(index));
        return pages[(int)index.Value].Reference;
    }
    public async Task<PageTextSnapshot> GetTextAsync(PageIndex page, CancellationToken cancellationToken = default)
    {
        var reference = await Page(page, cancellationToken).ConfigureAwait(false);
        return Wire.Read<PageTextSnapshot>((await Call(4, new { page = reference }, cancellationToken).ConfigureAwait(false)).Result);
    }
    public async Task<PageGeometrySnapshot> GetGeometryAsync(PageIndex page, CancellationToken cancellationToken = default)
    {
        var reference = await Page(page, cancellationToken).ConfigureAwait(false);
        return Wire.Read<PageGeometrySnapshot>((await Call(5, new { page = reference }, cancellationToken).ConfigureAwait(false)).Result);
    }
    public async Task<RgbaImage> RenderAsync(PageIndex page, RenderOptions options, CancellationToken cancellationToken = default)
    {
        if (!Enum.IsDefined(options.Background)) throw new ArgumentOutOfRangeException(nameof(options.Background));
        var reference = await Page(page, cancellationToken).ConfigureAwait(false);
        var args = new { page = reference, options = new { target = options.Region is { } region ? (object)new { kind = "rect", rect = region } : new { kind = "page" }, viewport = new { kind = "scale", options.Scale }, rotation = (int)options.Rotation, background = options.Background == RenderBackground.Transparent ? "transparent" : "white", options.IncludeAnnotations, options.IncludeFormFields, options.MaxOutputPixels } };
        var result = await Call(6, args, cancellationToken).ConfigureAwait(false);
        var raster = result.Result.GetProperty("raster");
        if (raster.GetProperty("color").GetString() != "rgba8" || raster.GetProperty("premultipliedAlpha").GetBoolean()) throw new JsonException("Expected straight-alpha RGBA8 raster");
        var width = raster.GetProperty("width").GetInt32(); var height = raster.GetProperty("height").GetInt32(); var stride = raster.GetProperty("stride").GetInt32();
        var payload = raster.GetProperty("data").GetProperty("payload").GetInt32();
        if (width <= 0 || height <= 0 || stride < checked(width * 4) || payload < 0 || payload >= result.Payloads.Length) throw new JsonException("Invalid raster layout");
        var bytes = result.Payloads[payload]; if (bytes.LongLength != (long)stride * height) throw new JsonException("Raster byte length disagrees with layout");
        return new(bytes, width, height, stride, Wire.Read<PageRect>(result.Result.GetProperty("area")));
    }
    public async Task<SearchSlice> SearchLiteralAsync(SearchOptions options, CancellationToken cancellationToken = default)
    {
        PageReference? from = options.StartingPage is { } page ? await Page(page, cancellationToken).ConfigureAwait(false) : null;
        var request = new { text = options.Text, options.MatchCase, options.WholeWord, options.MatchDiacritics, options.IgnoreWhitespace, options.Snippets, from, options.Cursor, limit = new { pages = options.MaxPages, matches = options.MaxMatches } };
        return Wire.Read<SearchSlice>((await Call(7, new { request }, cancellationToken).ConfigureAwait(false)).Result);
    }
    public ValueTask DisposeAsync()
    {
        lock (gate)
        {
            if (disposed == null) disposed = engine.CloseDocument(document);
            return new(disposed);
        }
    }
}
