using EmbedPDF.Engine;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Collections.Immutable;

var fonts = ImmutableArray.CreateBuilder<StartupFont>();
var fontManifest = Path.Combine(AppContext.BaseDirectory, "epdf-fonts.json");
using (var inventory = JsonDocument.Parse(await File.ReadAllTextAsync(fontManifest)))
{
    foreach (var font in inventory.RootElement.GetProperty("fonts").EnumerateArray())
    {
        var bytes = await File.ReadAllBytesAsync(Path.Combine(AppContext.BaseDirectory, font.GetProperty("path").GetString()!));
        fonts.Add(new(font.GetProperty("key").GetString()!, bytes) {
            FamilyName = font.GetProperty("familyName").GetString()!,
            Weight = font.GetProperty("weight").GetInt32(),
            Fallback = font.GetProperty("fallback").GetBoolean(),
            Italic = font.TryGetProperty("italic", out var italic) && italic.ValueKind != JsonValueKind.Null ? italic.GetBoolean() : null
        });
    }
}
await using var engine = await PdfEngine.CreateAsync(new() { StartupFonts = fonts.ToImmutable() });
if (AbiVersion() >> 16 != 1) throw new InvalidOperationException("Native ABI identity mismatch");
await using var document = await engine.OpenAsync(await File.ReadAllBytesAsync(args[0]));
var pages = await document.GetPagesAsync();
if (pages.Length == 0) throw new InvalidOperationException("PDF has no pages");
var text = await document.GetTextAsync(new(0));
if (text.CharCount == 0 || text.Text.Length == 0)
    throw new InvalidOperationException("Text-bearing corpus returned empty text");
var geometry = await document.GetGeometryAsync(new(0));
if (TextGeometry.SliceText(text, new(new(0), text.CharCount)) != text.Text)
    throw new InvalidOperationException("Helper text differs from page text");
_ = TextGeometry.GetSegments(geometry, new(new(0), text.CharCount));
_ = PageTransforms.Apply(new(pages[0].Size, PageRotation.None, 1, 1), [new(0, 0)]);
var image = await document.RenderAsync(new(0), new() {
    Region = new(0, 0, Math.Min(32, pages[0].Size.Width), Math.Min(32, pages[0].Size.Height)),
    Background = RenderBackground.Transparent });
if (image.Width <= 0 || image.Height <= 0 || image.Stride != image.Width * 4 ||
    image.Bytes.Length != image.Stride * image.Height)
    throw new InvalidOperationException("Invalid region RGBA frame");
Console.WriteLine($"PASS: ABI=1 pages={pages.Length} chars={text.CharCount} region={image.Width}x{image.Height}");

[DllImport("epdf_capi", EntryPoint = "epdf_abi_version", CallingConvention = CallingConvention.Cdecl)]
static extern uint AbiVersion();
