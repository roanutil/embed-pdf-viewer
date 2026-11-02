using System.Buffers.Binary;
using System.Collections.Immutable;
using System.Text.Json;
using EmbedPDF.Engine;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Xunit;
namespace EmbedPDF.Tests;

public class VectorTests
{
    static JsonElement Vector(string relative) => JsonDocument.Parse(File.ReadAllText(Path.Combine(FacadeTests.Root, "tooling/engine-conformance/vectors", relative))).RootElement;
    static byte[] Frame(object value, params byte[][] payloads)
    {
        var json = Wire.Encode(value); var frame = new byte[checked(4 + json.Length + 4 + payloads.Sum(p => 4 + p.Length))]; BinaryPrimitives.WriteUInt32LittleEndian(frame, (uint)json.Length); json.CopyTo(frame, 4); var at = 4 + json.Length; BinaryPrimitives.WriteUInt32LittleEndian(frame.AsSpan(at), (uint)payloads.Length); at += 4;
        foreach (var bytes in payloads) { BinaryPrimitives.WriteUInt32LittleEndian(frame.AsSpan(at), (uint)bytes.Length); at += 4; bytes.CopyTo(frame, at); at += bytes.Length; }
        return frame;
    }
    [Fact]
    public void IndexUnitsRejectImplicitAssignmentAndWrongHelperArguments()
    {
        var source = "using EmbedPDF.Engine; class Misuse { void Bad(PageTextSnapshot text) { PdfCharIndex character = new Utf16Offset(1); Utf16Offset offset = new PdfCharIndex(1); TextGeometry.GetTextOffset(text, new Utf16Offset(1)); } }";
        var references = ((string)AppContext.GetData("TRUSTED_PLATFORM_ASSEMBLIES")!).Split(Path.PathSeparator).Select(path => MetadataReference.CreateFromFile(path)).Append(MetadataReference.CreateFromFile(typeof(PdfEngine).Assembly.Location));
        var compilation = CSharpCompilation.Create("Misuse", [CSharpSyntaxTree.ParseText(source)], references, new CSharpCompilationOptions(OutputKind.DynamicallyLinkedLibrary));
        var errors = compilation.GetDiagnostics().Where(d => d.Severity == DiagnosticSeverity.Error).ToArray();
        Assert.Equal(3, errors.Length); Assert.Contains(errors, e => e.Id == "CS0029"); Assert.Contains(errors, e => e.Id == "CS1503");
    }
    [Fact]
    public void TextSnapshotsAndAllCharmapHelpersConsumeCommittedVectors()
    {
        foreach (var c in Vector("text/charmap.json").GetProperty("cases").EnumerateArray())
        {
            var text = new PageTextSnapshot { Text = c.GetProperty("text").GetString()!, CharCount = c.GetProperty("charCount").GetUInt32(), CharMap = c.TryGetProperty("charMap", out var map) ? map.EnumerateArray().Select(a => new CharMapAnchor(new(a[0].GetUInt32()), new(a[1].GetUInt32()))).ToImmutableArray() : null };
            var roundtrip = Wire.Read<PageTextSnapshot>(JsonSerializer.SerializeToElement(text, Wire.Json)); Assert.Equal(text.Text, roundtrip.Text); if (text.CharMap is { } anchors) Assert.Equal(anchors.ToArray(), roundtrip.CharMap!.Value.ToArray()); else Assert.Null(roundtrip.CharMap);
            uint boundary = 0; foreach (var expected in c.GetProperty("boundaryTextOffsets").EnumerateArray()) Assert.Equal(new Utf16Offset(expected.GetUInt32()), TextGeometry.GetTextOffset(text, new(boundary++)));
            foreach (var range in c.GetProperty("charRanges").EnumerateArray())
            {
                var actual = TextGeometry.GetCharacterRange(text, new(range.GetProperty("start").GetUInt32()), new(range.GetProperty("end").GetUInt32())); var expected = range.GetProperty("range"); var start = expected.GetProperty("start").GetUInt32(); var end = expected.GetProperty("end").GetUInt32(); Assert.Equal(new PdfCharRange(new(start), end - start), actual);
            }
            if (c.TryGetProperty("slices", out var slices)) foreach (var slice in slices.EnumerateArray()) Assert.Equal(slice.GetProperty("text").GetString(), TextGeometry.SliceText(text, new(new(slice.GetProperty("start").GetUInt32()), slice.GetProperty("end").GetUInt32() >= slice.GetProperty("start").GetUInt32() ? slice.GetProperty("end").GetUInt32() - slice.GetProperty("start").GetUInt32() : 0)));
        }
    }
    [Fact]
    public void GeometryVariantsAndLayoutProbesConsumeCommittedVectors()
    {
        bool oriented = false, upright = false, empty = false, space = false;
        foreach (var c in Vector("text/layout.json").GetProperty("cases").EnumerateArray())
        {
            var geometry = Vector("pages.geometry/" + c.GetProperty("case").GetString() + ".json");
            foreach (var page in c.GetProperty("pages").EnumerateArray())
            {
                var snapshot = geometry.GetProperty("pages").EnumerateArray().Single(p => p.GetProperty("index").GetUInt32() == page.GetProperty("index").GetUInt32()).GetProperty("snapshot");
                var decoded = Wire.Read<PageGeometrySnapshot>(snapshot);
                foreach (var run in decoded.Runs) switch (run) { case UprightRun u: upright = true; empty |= u.Glyphs.Any(g => g.Empty); space |= u.Glyphs.Any(g => g.Space); break; case OrientedRun o: oriented = true; empty |= o.Glyphs.Any(g => g.Empty); space |= o.Glyphs.Any(g => g.Space); break; default: throw new Exception("Unknown geometry variant"); }
                foreach (var probe in page.GetProperty("layout").GetProperty("glyphs").EnumerateArray())
                {
                    var quad = TextGeometry.GetCharacterQuad(decoded, new(probe.GetProperty("glyph").GetUInt32()));
                    AssertGeometry(JsonSerializer.SerializeToElement(quad, Wire.Json), probe.GetProperty("quad"));
                }
                foreach (var probe in page.GetProperty("layout").GetProperty("segments").EnumerateArray())
                {
                    var segments = TextGeometry.GetSegments(decoded, new(new(probe.GetProperty("start").GetUInt32()), probe.GetProperty("count").GetUInt32()));
                    AssertGeometry(JsonSerializer.SerializeToElement(segments, Wire.Json), probe.GetProperty("segments"));
                }
            }
        }
        Assert.True(oriented && upright && empty && space);
    }
    static void AssertGeometry(JsonElement actual, JsonElement expected)
    {
        if (expected.ValueKind == JsonValueKind.Number) { Assert.Equal(expected.GetDouble(), Math.Floor(actual.GetDouble() * 10000 + 0.5) / 10000); return; }
        if (expected.ValueKind == JsonValueKind.Object) { foreach (var p in expected.EnumerateObject()) AssertGeometry(actual.GetProperty(p.Name), p.Value); return; }
        if (expected.ValueKind == JsonValueKind.Array) { Assert.Equal(expected.GetArrayLength(), actual.GetArrayLength()); for (int i = 0; i < expected.GetArrayLength(); i++) AssertGeometry(actual[i], expected[i]); return; }
        Assert.Equal(expected.GetRawText(), actual.GetRawText());
    }
    [Fact]
    public void FramesRejectTruncationOverflowAndUnknownVariants()
    {
        var frame = Frame(new { result = new { value = 1 } }, new byte[] { 1, 2, 3 }); Assert.Equal(new byte[] { 1, 2, 3 }, Wire.Decode(frame).Payloads[0]);
        for (var length = 0; length < frame.Length; length++) Assert.Throws<JsonException>(() => Wire.Decode(frame.AsSpan(0, length)));
        Assert.Throws<JsonException>(() => Wire.Decode(frame.Concat(new byte[] { 0 }).ToArray()));
        Assert.Throws<JsonException>(() => Wire.Decode(new byte[] { 255, 255, 255, 255 }));
        Assert.Throws<JsonException>(() => Wire.Read<PageGeometrySnapshot>(JsonDocument.Parse("{\"runs\":[{\"kind\":\"future\"}]}").RootElement));
        var nullText = Wire.Read<PageTextSnapshot>(JsonDocument.Parse("{\"text\":\"\",\"charCount\":0}").RootElement); var emptyText = Wire.Read<PageTextSnapshot>(JsonDocument.Parse("{\"text\":\"\",\"charCount\":0,\"charMap\":[]}").RootElement); Assert.Null(nullText.CharMap); Assert.Empty(emptyText.CharMap!.Value);
    }
    [Fact]
    public void WireVectorsPinSupportedOperationFieldsAndSearchSliceDto()
    {
        var wire = Vector("wire/requests.json");
        var operations = wire.ValueKind == JsonValueKind.Array ? wire : wire.GetProperty("cases");
        foreach (var kind in new[] { "open.fatMem", "close", "pages.list", "pages.text", "pages.geometry", "pages.render", "search.query" }) Assert.Contains(operations.EnumerateArray(), entry => entry.GetProperty("kind").GetString() == kind);
        foreach (var c in Vector("search/literal.json").GetProperty("cases").EnumerateArray())
        {
            var matches = c.GetProperty("matches").EnumerateArray().Select(m => new { page = new { kind = "objectNumber", objectNumber = 3 }, start = m.GetProperty("start").GetUInt32(), count = m.GetProperty("count").GetUInt32(), segments = Array.Empty<object>(), snippet = (object?)null }).ToArray();
            var decoded = Wire.Read<SearchSlice>(JsonSerializer.SerializeToElement(new { matches, nextCursor = (string?)null, pagesSearched = 1, pageCount = 1 }, Wire.Json)); Assert.Equal(matches.Length, decoded.Matches.Length); Assert.Null(decoded.NextCursor);
            for (int i = 0; i < matches.Length; i++) { Assert.Equal(matches[i].start, decoded.Matches[i].Start.Value); Assert.Equal(matches[i].count, decoded.Matches[i].Count); Assert.Null(decoded.Matches[i].Snippet); }
        }
    }
}
