using System.Collections.Immutable;
using System.Text.Json;
using System.Text.Json.Serialization;
namespace EmbedPDF.Engine;

[JsonConverter(typeof(IndexConverter<PageIndex>))]
public readonly record struct PageIndex(uint Value);
[JsonConverter(typeof(IndexConverter<PdfCharIndex>))]
public readonly record struct PdfCharIndex(uint Value);
[JsonConverter(typeof(IndexConverter<Utf16Offset>))]
public readonly record struct Utf16Offset(uint Value);
public readonly record struct PdfCharRange(PdfCharIndex Start, uint Count);
public readonly record struct PagePoint(double X, double Y);
public readonly record struct PageSize(double Width, double Height);
public readonly record struct PageRect(double X, double Y, double Width, double Height);
public readonly record struct PdfBox(double Left, double Bottom, double Right, double Top);
public readonly record struct PageQuad(PagePoint UpperLeft, PagePoint UpperRight, PagePoint LowerLeft, PagePoint LowerRight);
[JsonConverter(typeof(RotationConverter))]
public enum PageRotation { None = 0, Clockwise90 = 90, Clockwise180 = 180, Clockwise270 = 270 }
public sealed record PageReference(string Kind, uint ObjectNumber);
public sealed record PageBoxes(PageRect Media, PageRect Crop, PageRect? Bleed, PageRect? Trim, PageRect? Art);
public sealed record PageInfo(PageIndex Index, [property: JsonPropertyName("ref")] PageReference Reference, string? Label, PageSize Size, PageRotation Rotation, double UserUnit, PageBoxes Boxes, PdfBox PdfCropBox, string? TabOrder, JsonElement? Actions);
public sealed record CharMapAnchor([property: JsonPropertyName("char")] PdfCharIndex Character, [property: JsonPropertyName("text")] Utf16Offset Text);
public sealed record PageTextSnapshot
{
    public required string Text { get; init; }
    public required uint CharCount { get; init; }
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] public ImmutableArray<CharMapAnchor>? CharMap { get; init; }
}
public sealed record UprightGlyph(PageRect Loose, PageRect? Tight, bool Space, bool Empty);
public sealed record OrientedGlyph(PageQuad Loose, PageQuad? Tight, bool Space, bool Empty);
[JsonConverter(typeof(GeometryRunConverter))]
public abstract record GeometryRun([property: JsonRequired] PageRect Rect, [property: JsonRequired] PdfCharIndex Start, double? FontSize);
public sealed record UprightRun(PageRect Rect, PdfCharIndex Start, double? FontSize, [property: JsonRequired] ImmutableArray<UprightGlyph> Glyphs) : GeometryRun(Rect, Start, FontSize);
public sealed record OrientedRun(PageRect Rect, PdfCharIndex Start, double? FontSize, [property: JsonRequired] ImmutableArray<OrientedGlyph> Glyphs, [property: JsonRequired] double Rotation, [property: JsonRequired] bool AscentFlip) : GeometryRun(Rect, Start, FontSize);
public sealed record PageGeometrySnapshot([property: JsonRequired] ImmutableArray<GeometryRun> Runs);
public sealed record TextSegment(PageQuad Quad, PageRect Rect, sbyte Advance);
public sealed record SearchSnippet(string Before, [property: JsonPropertyName("match")] string Match, string After);
public sealed record SearchMatch(PageReference Page, PdfCharIndex Start, uint Count, ImmutableArray<TextSegment> Segments, SearchSnippet? Snippet);
public sealed record SearchSlice(ImmutableArray<SearchMatch> Matches, string? NextCursor, uint PagesSearched, uint PageCount);
public sealed record SearchOptions(string Text)
{
    public bool MatchCase { get; init; }
    public bool WholeWord { get; init; }
    public bool MatchDiacritics { get; init; }
    public bool IgnoreWhitespace { get; init; }
    public bool Snippets { get; init; }
    public PageIndex? StartingPage { get; init; }
    public string? Cursor { get; init; }
    public uint MaxPages { get; init; } = 64;
    public uint MaxMatches { get; init; } = 256;
}
public enum RenderBackground { White, Transparent }
public sealed record RenderOptions
{
    public PageRect? Region { get; init; }
    public double Scale { get; init; } = 1;
    public PageRotation Rotation { get; init; }
    public RenderBackground Background { get; init; }
    public bool? IncludeAnnotations { get; init; }
    public bool? IncludeFormFields { get; init; }
    public ulong MaxOutputPixels { get; init; } = 16_777_216;
}
public sealed class RgbaImage
{
    internal RgbaImage(byte[] bytes, int width, int height, int stride, PageRect area) { Bytes = bytes; Width = width; Height = height; Stride = stride; Area = area; }
    public ReadOnlyMemory<byte> Bytes { get; }
    public int Width { get; }
    public int Height { get; }
    public int Stride { get; }
    public PageRect Area { get; }
}
public sealed record StartupFont(string Key, ReadOnlyMemory<byte> Bytes)
{
    public string FamilyName { get; init; } = "";
    public int Weight { get; init; }
    public bool? Italic { get; init; }
    public bool Fallback { get; init; }
}
public sealed record NativeLimits
{
    public uint MaxPendingJobs { get; init; } = 16;
    public ulong MaxJsonBytes { get; init; } = 8 * 1024 * 1024;
    public ulong MaxPayloadBytes { get; init; } = 256 * 1024 * 1024;
    public ulong MaxQueuedInputBytes { get; init; } = 256 * 1024 * 1024;
    public ulong MaxRetainedInputBytes { get; init; } = 1024 * 1024 * 1024;
    public ulong MaxResultBytes { get; init; } = 256 * 1024 * 1024;
    public ulong MaxRetainedResultBytes { get; init; } = 512 * 1024 * 1024;
    public ulong MaxOutputPixels { get; init; } = 16_777_216;
}
public sealed record PdfEngineOptions
{
    public int MaxPendingRequests { get; init; } = 16;
    public long MaxQueuedBytes { get; init; } = 256 * 1024 * 1024;
    public NativeLimits Limits { get; init; } = new();
    public ImmutableArray<StartupFont> StartupFonts { get; init; } = [];
}
public sealed record PageTransformInput(PageSize PageSize, PageRotation Rotation, double Scale, double Dpr, double? BaseScale = null);
public sealed record TransformMetrics(double ViewWidth, double ViewHeight, double ContentWidth, double ContentHeight, double DeviceWidth, double DeviceHeight, double RenderScale, double ViewScale, double BaseScale, double Zoom, PageRotation Rotation, string CssMatrix);
public sealed record PageTransformResult(TransformMetrics Transform, ImmutableArray<PagePoint> PageToView, ImmutableArray<PageRect> PageRectsToView, ImmutableArray<PagePoint> ViewToPage, ImmutableArray<PageRect> ViewRectsToPage);
public sealed class PdfException : Exception
{
    public string Code { get; }
    public uint? NativeStatus { get; }
    internal PdfException(string code, string message, uint? status = null) : base(message) { Code = code; NativeStatus = status; }
}

internal sealed class IndexConverter<T> : JsonConverter<T> where T : struct
{
    public override T Read(ref Utf8JsonReader reader, Type type, JsonSerializerOptions options) => (T)(object)(typeof(T) == typeof(PageIndex) ? (object)new PageIndex(reader.GetUInt32()) : typeof(T) == typeof(PdfCharIndex) ? new PdfCharIndex(reader.GetUInt32()) : new Utf16Offset(reader.GetUInt32()));
    public override void Write(Utf8JsonWriter writer, T value, JsonSerializerOptions options) => writer.WriteNumberValue(value switch { PageIndex i => i.Value, PdfCharIndex i => i.Value, Utf16Offset i => i.Value, _ => throw new JsonException("Unknown index unit") });
}
internal sealed class GeometryRunConverter : JsonConverter<GeometryRun>
{
    public override GeometryRun Read(ref Utf8JsonReader reader, Type type, JsonSerializerOptions options)
    {
        using var doc = JsonDocument.ParseValue(ref reader); var json = doc.RootElement;
        if (json.TryGetProperty("kind", out _)) throw new JsonException("Unknown required geometry run variant");
        if (json.TryGetProperty("rotation", out _)) return json.Deserialize<OrientedRun>(options) ?? throw new JsonException("Missing oriented run");
        return json.Deserialize<UprightRun>(options) ?? throw new JsonException("Missing upright run");
    }
    public override void Write(Utf8JsonWriter writer, GeometryRun value, JsonSerializerOptions options)
    {
        if (value is OrientedRun oriented) JsonSerializer.Serialize(writer, oriented, options);
        else if (value is UprightRun upright) JsonSerializer.Serialize(writer, upright, options);
        else throw new JsonException("Unknown required geometry run variant");
    }
}

internal sealed class RotationConverter : JsonConverter<PageRotation>
{
    public override PageRotation Read(ref Utf8JsonReader reader, Type type, JsonSerializerOptions options)
    {
        var rotation = reader.GetDouble();
        return rotation switch { 0 => PageRotation.None, 90 => PageRotation.Clockwise90, 180 => PageRotation.Clockwise180, 270 => PageRotation.Clockwise270, _ => throw new JsonException("Expected quarter-turn rotation") };
    }
    public override void Write(Utf8JsonWriter writer, PageRotation value, JsonSerializerOptions options)
    {
        if (!Enum.IsDefined(value)) throw new JsonException("Expected quarter-turn rotation");
        writer.WriteNumberValue((int)value);
    }
}
