using System.Collections.Immutable;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;
using EmbedPDF.Engine;

internal static class NativeSnapshot
{
    static readonly JsonSerializerOptions Json = new() { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };
    internal static async Task Run(string root, string requestPath, string outputPath)
    {
        var native = Environment.GetEnvironmentVariable("EPDF_NATIVE_DIR") ?? throw new InvalidOperationException("EPDF_NATIVE_DIR must point to the locally built native library");
        if (!Path.IsPathFullyQualified(native)) throw new ArgumentException("EPDF_NATIVE_DIR must be absolute");
        NativeLibrary.SetDllImportResolver(typeof(PdfEngine).Assembly, (name, _, _) =>
            name == "epdf_capi" ? NativeLibrary.Load(Path.Combine(native, OperatingSystem.IsWindows() ? "epdf_capi.dll" : OperatingSystem.IsMacOS() ? "libepdf_capi.dylib" : "libepdf_capi.so")) : IntPtr.Zero);
        using var source = JsonDocument.Parse(File.ReadAllBytes(requestPath));
        var request = source.RootElement;
        var fonts = request.GetProperty("fonts").EnumerateArray().Select(font =>
            new StartupFont(font.GetProperty("key").GetString()!, File.ReadAllBytes(Path.Combine(root, font.GetProperty("path").GetString()!)))
            {
                FamilyName = font.GetProperty("familyName").GetString()!,
                Weight = font.GetProperty("weight").GetInt32(),
                Italic = font.GetProperty("italic").GetBoolean(),
                Fallback = font.GetProperty("fallback").GetBoolean()
            }).ToImmutableArray();
        Console.Error.WriteLine("native parity: create engine with pinned ordered fonts");
        await using var engine = await PdfEngine.CreateAsync(new() { StartupFonts = fonts });
        Console.Error.WriteLine("native parity: open corpus PDF");
        await using var document = await engine.OpenAsync(File.ReadAllBytes(Path.Combine(root, request.GetProperty("pdf").GetString()!)));
        var results = new List<object>();
        foreach (var probe in request.GetProperty("probes").EnumerateArray())
        {
            var operation = probe.GetProperty("operation").GetString()!;
            var page = new PageIndex(probe.TryGetProperty("page", out var pi) ? pi.GetUInt32() : 0);
            Console.Error.WriteLine(JsonSerializer.Serialize(new { id = probe.GetProperty("id").GetString(), operation, status = "running" }));
            object? value;
            switch (operation)
            {
                case "pages":
                    value = (await document.GetPagesAsync()).Select(p => new { p.Index, p.Reference, p.Label, p.Size, p.Rotation, p.UserUnit, p.Boxes, p.PdfCropBox, p.TabOrder, p.Actions })
                        .Select(p => new Dictionary<string, object?> { ["index"] = p.Index, ["ref"] = p.Reference, ["label"] = p.Label, ["size"] = p.Size, ["rotation"] = p.Rotation, ["userUnit"] = p.UserUnit, ["boxes"] = p.Boxes, ["pdfCropBox"] = p.PdfCropBox, ["tabOrder"] = p.TabOrder, ["actions"] = p.Actions }).ToArray();
                    break;
                case "text": value = await document.GetTextAsync(page); break;
                case "geometry": value = await document.GetGeometryAsync(page); break;
                case "segments":
                    value = TextGeometry.GetSegments(await document.GetGeometryAsync(page), new(new(probe.GetProperty("start").GetUInt32()), probe.GetProperty("count").GetUInt32()));
                    break;
                case "slice":
                    value = TextGeometry.SliceText(await document.GetTextAsync(page), new(new(probe.GetProperty("start").GetUInt32()), probe.GetProperty("count").GetUInt32()));
                    break;
                case "transform":
                    var input = probe.GetProperty("input").Deserialize<PageTransformInput>(Json)!;
                    value = PageTransforms.Apply(input, Read<PagePoint>(probe, "pagePoints"), Read<PageRect>(probe, "pageRects"), Read<PagePoint>(probe, "viewPoints"), Read<PageRect>(probe, "viewRects"));
                    break;
                case "render":
                    var image = await document.RenderAsync(page, new() {
                        Region = probe.TryGetProperty("region", out var region) ? region.Deserialize<PageRect>(Json) : null,
                        Rotation = (PageRotation)probe.GetProperty("rotation").GetInt32(),
                        Scale = probe.GetProperty("scale").GetDouble(),
                        Background = RenderBackground.Transparent,
                        IncludeAnnotations = false, IncludeFormFields = false
                    });
                    value = new { image.Width, image.Height, image.Stride, image.Area, color = "rgba8", premultipliedAlpha = false, pixels = Convert.ToBase64String(image.Bytes.Span), sha256 = Convert.ToHexStringLower(SHA256.HashData(image.Bytes.Span)) };
                    break;
                case "helperText":
                    var text = probe.GetProperty("text").Deserialize<PageTextSnapshot>(Json)!;
                    value = new {
                        offsets = Enumerable.Range(0, checked((int)text.CharCount + 1)).Select(i => TextGeometry.GetTextOffset(text, new((uint)i))),
                        slices = probe.GetProperty("ranges").EnumerateArray().Select(r => TextGeometry.SliceText(text, new(new(r.GetProperty("start").GetUInt32()), r.GetProperty("count").GetUInt32()))).ToArray()
                    };
                    break;
                default: throw new ArgumentException($"Unknown parity operation {operation}");
            }
            results.Add(new { id = probe.GetProperty("id").GetString(), operation, value });
            await File.WriteAllTextAsync(outputPath, JsonSerializer.Serialize(results, Json));
        }
        await File.WriteAllTextAsync(outputPath, JsonSerializer.Serialize(results, Json));
    }
    static T[] Read<T>(JsonElement probe, string key) => probe.TryGetProperty(key, out var value) ? value.Deserialize<T[]>(Json)! : [];
}
