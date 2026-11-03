using System.Globalization;
using System.Numerics;
using System.Text.RegularExpressions;
using System.Text.Json;

// Compare the serialized tokens. No double conversion or intermediate rounding.
internal static class DecimalParity
{
    internal static List<string> Compare(JsonElement viewer, JsonElement native)
    {
        var errors = new List<string>();
        Visit(viewer, native, "$", errors);
        return errors;
    }
    static void Visit(JsonElement left, JsonElement right, string path, List<string> errors)
    {
        if (left.ValueKind != right.ValueKind) { errors.Add($"{path}: JSON kinds differ"); return; }
        switch (left.ValueKind)
        {
            case JsonValueKind.Number:
                var a = ParseExact(left.GetRawText(), path);
                var b = ParseExact(right.GetRawText(), path);
                if (decimal.Round(a, 4, MidpointRounding.AwayFromZero) != decimal.Round(b, 4, MidpointRounding.AwayFromZero))
                    errors.Add($"{path}: viewer={left.GetRawText()} native={right.GetRawText()} after decimal round(4, AwayFromZero)");
                break;
            case JsonValueKind.Object:
                var lp = Properties(left, path); var rp = Properties(right, path);
                foreach (var key in lp.Keys.Union(rp.Keys).Order(StringComparer.Ordinal))
                {
                    if (!lp.ContainsKey(key) || !rp.ContainsKey(key)) errors.Add($"{path}.{key}: property missing");
                    else Visit(lp[key], rp[key], $"{path}.{key}", errors);
                }
                break;
            case JsonValueKind.Array:
                if (left.GetArrayLength() != right.GetArrayLength()) { errors.Add($"{path}: array lengths differ"); break; }
                for (var i = 0; i < left.GetArrayLength(); i++) Visit(left[i], right[i], $"{path}[{i}]", errors);
                break;
            case JsonValueKind.String:
                if (!string.Equals(left.GetString(), right.GetString(), StringComparison.Ordinal)) errors.Add($"{path}: strings differ");
                break;
            default:
                if (left.GetRawText() != right.GetRawText()) errors.Add($"{path}: values differ");
                break;
        }
    }
    static decimal ParseExact(string token, string path)
    {
        if (!decimal.TryParse(token, NumberStyles.Float, CultureInfo.InvariantCulture, out var value))
            throw new JsonException($"{path}: number outside decimal range");
        var match = Regex.Match(token, @"^(-?)(\d+)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$");
        if (!match.Success || !int.TryParse(match.Groups[4].Success ? match.Groups[4].Value : "0", out var exponent) || Math.Abs((long)exponent) > 1000)
            throw new JsonException($"{path}: unsupported decimal token");
        var coefficient = BigInteger.Parse(match.Groups[2].Value + match.Groups[3].Value, CultureInfo.InvariantCulture);
        var scale = match.Groups[3].Length - exponent;
        var bits = decimal.GetBits(value);
        var parsed = (BigInteger)(uint)bits[0] | ((BigInteger)(uint)bits[1] << 32) | ((BigInteger)(uint)bits[2] << 64);
        var parsedScale = (bits[3] >> 16) & 255;
        var difference = parsedScale - scale;
        if (Math.Abs((long)difference) > 1000
            || (difference >= 0 ? coefficient * BigInteger.Pow(10, difference) != parsed : coefficient != parsed * BigInteger.Pow(10, -difference)))
            throw new JsonException($"{path}: decimal parsing would lose serialized precision");
        return value;
    }
    static Dictionary<string, JsonElement> Properties(JsonElement value, string path)
    {
        var result = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
        foreach (var p in value.EnumerateObject())
            if (!result.TryAdd(p.Name, p.Value)) throw new JsonException($"{path}: duplicate property {p.Name}");
        return result;
    }
}
