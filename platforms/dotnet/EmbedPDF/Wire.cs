using System.Buffers.Binary;
using System.Collections.Immutable;
using System.Text.Json;
using System.Text.Json.Serialization;
namespace EmbedPDF.Engine;

internal sealed record WireResult(JsonElement Result, ImmutableArray<byte[]> Payloads, ulong Document = 0);
internal static class Wire
{
    internal static readonly JsonSerializerOptions Json = new() { PropertyNamingPolicy = JsonNamingPolicy.CamelCase, DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull };
    internal static byte[] Encode(object value) => JsonSerializer.SerializeToUtf8Bytes(value, Json);
    internal static T Read<T>(JsonElement value) => value.Deserialize<T>(Json) ?? throw new JsonException($"Missing {typeof(T).Name}");
    internal static byte[] PayloadFrame(IReadOnlyList<ReadOnlyMemory<byte>> payloads)
    {
        if (payloads.Count > 64) throw new ArgumentOutOfRangeException(nameof(payloads));
        var length = 4; foreach (var payload in payloads) length = checked(length + 4 + payload.Length);
        var bytes = new byte[length]; BinaryPrimitives.WriteUInt32LittleEndian(bytes, (uint)payloads.Count); var offset = 4;
        foreach (var payload in payloads) { BinaryPrimitives.WriteUInt32LittleEndian(bytes.AsSpan(offset), (uint)payload.Length); offset += 4; payload.Span.CopyTo(bytes.AsSpan(offset)); offset += payload.Length; }
        return bytes;
    }
    internal static WireResult Decode(ReadOnlySpan<byte> frame)
    {
        var offset = 0;
        // Keep each read checked before slicing; native lengths are untrusted at this boundary.
        var jsonLength = TakeLength(frame, ref offset); if (jsonLength > frame.Length - offset) throw new JsonException("Truncated JSON frame");
        using var document = JsonDocument.Parse(frame.Slice(offset, jsonLength).ToArray()); offset += jsonLength;
        var count = TakeLength(frame, ref offset); if (count > 64) throw new JsonException("Too many payloads");
        var payloads = ImmutableArray.CreateBuilder<byte[]>(count);
        for (var i = 0; i < count; i++) { var length = TakeLength(frame, ref offset); if (length > frame.Length - offset) throw new JsonException("Truncated payload frame"); payloads.Add(frame.Slice(offset, length).ToArray()); offset += length; }
        if (offset != frame.Length) throw new JsonException("Trailing response frame bytes");
        var envelope = document.RootElement;
        if (envelope.TryGetProperty("error", out var error)) throw Error(error);
        if (!envelope.TryGetProperty("result", out var result)) throw new JsonException("Missing result envelope");
        return new(result.Clone(), payloads.MoveToImmutable());
    }
    static int TakeLength(ReadOnlySpan<byte> frame, ref int offset)
    {
        if (frame.Length - offset < 4) throw new JsonException("Truncated frame length");
        var length = BinaryPrimitives.ReadUInt32LittleEndian(frame[offset..]); offset += 4;
        if (length > int.MaxValue) throw new JsonException("Frame length exceeds managed bounds"); return (int)length;
    }
    static Exception Error(JsonElement error)
    {
        var code = error.TryGetProperty("code", out var c) ? c.GetString() ?? "Unknown" : "Unknown";
        var message = error.TryGetProperty("message", out var m) ? m.GetString() ?? code : code;
        return code == "Aborted" ? new OperationCanceledException(message) : new PdfException(code, message);
    }
    internal static void Status(uint status)
    {
        if (status == 0) return;
        var code = status switch { 1 => "Timeout", 2 => "InvalidArgument", 3 => "InvalidHandle", 4 => "Closed", 5 => "Busy", 6 => "LimitExceeded", 7 => "Panicked", 8 => "RuntimeUnavailable", 9 => "AlreadyTaken", _ => "UnknownStatus" };
        throw new PdfException(code, $"Native operation failed: {code} ({status})", status);
    }
    internal static unsafe WireResult CopyResult(ulong id, uint status = 0)
    {
        if (id == 0) { Status(status); throw new JsonException("Native reply has no result"); }
        using var result = new ResultHandle(id); using var lease = result.Acquire();
        Status(NativeMethods.View(lease.Value, out var pointer, out var length));
        if (length > int.MaxValue || (pointer == null && length != 0)) throw new JsonException("Invalid native result view");
        WireResult value;
        try { value = Decode(new ReadOnlySpan<byte>(pointer, (int)length)); }
        catch (PdfException error) when (status != 0) { throw new PdfException(error.Code, error.Message, status); }
        Status(status); return value;
    }
    internal static unsafe WireResult Helper(uint operation, object args)
    {
        var json = Encode(args); uint status; ulong result;
        fixed (byte* pointer = json) status = NativeMethods.Helper(operation, pointer, (ulong)json.Length, out result);
        return CopyResult(result, status);
    }
}
