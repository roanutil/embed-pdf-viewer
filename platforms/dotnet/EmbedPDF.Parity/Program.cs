using System.Text.Json;
if (args is ["self-test"])
{
    var probes = new (string Id, string Left, string Right, bool Equal)[] {
        ("positive-midpoint", "0.00005", "0.0001", true),
        ("negative-midpoint", "-0.00005", "-0.0001", true),
        ("midpoint-mismatch", "0.00005", "0", false),
        ("negative-midpoint-mismatch", "-0.00005", "0", false),
        ("round-once", "0.000049999", "0", true),
        ("decimal-not-double", "1.23445", "1.2345", true),
        ("exponent", "5e-5", "0.0001", true),
        ("crop-rotation-boundary", "{\"x\":-12.00005,\"rotation\":90}", "{\"x\":-12,\"rotation\":90}", false),
        ("rotation-mismatch", "{\"rotation\":90}", "{\"rotation\":270}", false),
        ("surrogate-dropped-combining", "{\"text\":\"a😀é\",\"charMap\":[{\"char\":1,\"text\":1},{\"char\":2,\"text\":3},{\"char\":3,\"text\":3}]}", "{\"text\":\"a😀é\",\"charMap\":[{\"char\":1,\"text\":1},{\"char\":2,\"text\":3},{\"char\":3,\"text\":3}]}", true),
        ("text-mismatch", "\"é\"", "\"é\"", false),
        ("empty-array-not-null", "[]", "null", false),
        ("dropped-map-mismatch", "{\"charCount\":3,\"text\":\"ab\"}", "{\"charCount\":2,\"text\":\"ab\"}", false),
        ("object-order", "{\"x\":1,\"y\":2}", "{\"y\":2,\"x\":1}", true),
        ("array-order", "[1,2]", "[2,1]", false)
    };
    using (var precise = JsonDocument.Parse("0.00004999999999999999999999999999999999"))
    using (var zero = JsonDocument.Parse("0"))
    {
        var rejected = false;
        try { DecimalParity.Compare(precise.RootElement, zero.RootElement); } catch (JsonException) { rejected = true; }
        if (!rejected) throw new Exception("precision-loss-must-refuse");
        Console.WriteLine("{\"id\":\"decimal-precision-loss\",\"status\":\"passed\"}");
    }
    foreach (var p in probes) {
        using var l = JsonDocument.Parse(p.Left); using var r = JsonDocument.Parse(p.Right);
        var errors = DecimalParity.Compare(l.RootElement, r.RootElement);
        if ((errors.Count == 0) != p.Equal) throw new Exception($"{p.Id}: expected equal={p.Equal}; {string.Join(";", errors)}");
        Console.WriteLine(JsonSerializer.Serialize(new { id = p.Id, status = "passed", expectedEqual = p.Equal }));
    }
}
else if (args is ["compare", var left, var right])
{
    using var l = JsonDocument.Parse(File.ReadAllBytes(left)); using var r = JsonDocument.Parse(File.ReadAllBytes(right));
    var errors = DecimalParity.Compare(l.RootElement, r.RootElement);
    Console.WriteLine(JsonSerializer.Serialize(new { status = errors.Count == 0 ? "passed" : "failed", errors }));
    return errors.Count == 0 ? 0 : 1;
}
else if (args is ["snapshot", var root, var request, var output]) await NativeSnapshot.Run(root, request, output);
else throw new ArgumentException("Expected self-test or compare <viewer.json> <native.json>");
return 0;
