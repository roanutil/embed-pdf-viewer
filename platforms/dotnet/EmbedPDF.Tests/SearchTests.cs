using System.Text;
using EmbedPDF.Engine;
using Xunit;
namespace EmbedPDF.Tests;

public class SearchTests
{
    static byte[] FourPages()
    {
        var source = new StringBuilder("%PDF-1.4\n"); var offsets = new List<int> { 0 };
        void Object(int id, string value) { offsets.Add(Encoding.ASCII.GetByteCount(source.ToString())); source.Append($"{id} 0 obj\n{value}\nendobj\n"); }
        Object(1, "<< /Type /Catalog /Pages 2 0 R >>"); Object(2, "<< /Type /Pages /Kids [3 0 R 5 0 R 7 0 R 9 0 R] /Count 4 >>");
        for (int i = 0; i < 4; i++) { var text = $"BT /F1 12 Tf 10 60 Td (needle page {i}) Tj ET"; Object(3 + i * 2, $"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 11 0 R >> >> /Contents {4 + i * 2} 0 R >>"); Object(4 + i * 2, $"<< /Length {text.Length} >>\nstream\n{text}\nendstream"); }
        Object(11, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"); var xref = source.Length; source.Append($"xref\n0 {offsets.Count}\n0000000000 65535 f \n"); foreach (var offset in offsets.Skip(1)) source.Append($"{offset:D10} 00000 n \n"); source.Append($"trailer\n<< /Size {offsets.Count} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"); return Encoding.ASCII.GetBytes(source.ToString());
    }
    [Fact]
    public async Task SearchIsOneCancellableSliceAndCursorRejectsOtherQueryOrDocument()
    {
        await using var engine = await PdfEngine.CreateAsync(); await using var first = await engine.OpenAsync(FourPages()); await using var second = await engine.OpenAsync(FourPages());
        var options = new SearchOptions("needle") { Snippets = true, MaxPages = 1, MaxMatches = 1 };
        var slice = await first.SearchLiteralAsync(options); Assert.Equal(4u, slice.PageCount); Assert.Equal(1u, slice.PagesSearched); Assert.Single(slice.Matches); Assert.NotEmpty(slice.Matches[0].Segments); Assert.NotNull(slice.Matches[0].Snippet); Assert.NotNull(slice.NextCursor);
        Assert.Equal("InvalidArg", (await Assert.ThrowsAsync<PdfException>(() => first.SearchLiteralAsync(options with { Text = "other", Cursor = slice.NextCursor }))).Code);
        Assert.Equal("InvalidArg", (await Assert.ThrowsAsync<PdfException>(() => second.SearchLiteralAsync(options with { Cursor = slice.NextCursor }))).Code);
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => first.SearchLiteralAsync(options with { Cursor = slice.NextCursor }, new(true)));
        uint pages = 1; while (slice.NextCursor is { } cursor) { slice = await first.SearchLiteralAsync(options with { Cursor = cursor }); Assert.Equal(++pages, slice.PagesSearched); Assert.True(pages <= 4); }
        Assert.Equal(4u, pages);
    }
    [Fact]
    public async Task RepeatedResultsAndDocumentsReleaseNativeByteAndSlotBudgets()
    {
        await using var engine = await PdfEngine.CreateAsync(new() { MaxPendingRequests = 2, Limits = new() { MaxPendingJobs = 1, MaxResultBytes = 65536, MaxRetainedResultBytes = 65536, MaxRetainedInputBytes = (ulong)FacadeTests.Pdf.Length + 16 } });
        for (int i = 0; i < 40; i++) { await using var doc = await engine.OpenAsync(FacadeTests.Pdf); var text = await doc.GetTextAsync(new(0)); Assert.NotEmpty(text.Text); var geometry = await doc.GetGeometryAsync(new(0)); Assert.NotEmpty(geometry.Runs); }
        Assert.Equal(0, engine.PendingRequests); Assert.Equal(0, engine.QueuedBytes);
    }
}
