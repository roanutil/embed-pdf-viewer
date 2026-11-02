using System.Collections.Immutable;
namespace EmbedPDF.Engine;

public static class TextGeometry
{
    public static ImmutableArray<TextSegment> GetSegments(PageGeometrySnapshot geometry, PdfCharRange range)
        => Wire.Read<ImmutableArray<TextSegment>>(Wire.Helper(1, new { geometry, range }).Result);
    public static PageQuad? GetCharacterQuad(PageGeometrySnapshot geometry, PdfCharIndex index)
    {
        var result = Wire.Helper(2, new { geometry, index }).Result;
        return result.ValueKind == System.Text.Json.JsonValueKind.Null ? null : Wire.Read<PageQuad>(result);
    }
    public static string SliceText(PageTextSnapshot text, PdfCharRange range)
        => Wire.Read<string>(Wire.Helper(3, new { text, range = new { start = range.Start, end = checked(range.Start.Value + range.Count) } }).Result);
    public static PdfCharRange GetCharacterRange(PageTextSnapshot text, Utf16Offset start, Utf16Offset end)
    {
        var result = Wire.Helper(4, new { text, range = new { start, end } }).Result;
        var first = result.GetProperty("start").GetUInt32(); var last = result.GetProperty("end").GetUInt32();
        return new(new(first), checked(last - first));
    }
    public static Utf16Offset GetTextOffset(PageTextSnapshot text, PdfCharIndex boundary)
        => new(Wire.Helper(5, new { text, boundary }).Result.GetUInt32());
}
