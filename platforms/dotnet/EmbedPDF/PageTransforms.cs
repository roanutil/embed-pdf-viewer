namespace EmbedPDF.Engine;

public static class PageTransforms
{
    public static PageTransformResult Apply(PageTransformInput input, IReadOnlyList<PagePoint>? pagePoints = null, IReadOnlyList<PageRect>? pageRects = null, IReadOnlyList<PagePoint>? viewPoints = null, IReadOnlyList<PageRect>? viewRects = null)
        => Wire.Read<PageTransformResult>(Wire.Helper(6, new { input, pagePoints = pagePoints ?? [], pageRects = pageRects ?? [], viewPoints = viewPoints ?? [], viewRects = viewRects ?? [] }).Result);
}
