using System.Threading.Channels;
namespace EmbedPDF.Engine;

internal sealed class EngineDispatcher : IAsyncDisposable
{
    readonly object gate = new();
    readonly Channel<Request> requests;
    readonly PdfEngineOptions options;
    readonly TaskCompletionSource ready = new(TaskCreationOptions.RunContinuationsAsynchronously);
    readonly TaskCompletionSource stopped = new(TaskCreationOptions.RunContinuationsAsynchronously);
    EngineHandle? handle;
    bool closing;
    readonly AutoResetEvent wake = new(false);
    readonly Queue<Request> closes = new();
    long sequence;
    int pending;
    long queuedBytes;
    public int PendingRequests { get { lock (gate) return pending; } }
    public long QueuedBytes { get { lock (gate) return queuedBytes; } }
    sealed record Request(long Sequence, ulong Document, uint Operation, byte[] Args, byte[] Payloads, long Bytes, CancellationToken Cancellation, TaskCompletionSource<WireResult> Completion);

    EngineDispatcher(PdfEngineOptions options)
    {
        if (options.MaxPendingRequests <= 0 || options.MaxQueuedBytes <= 0) throw new ArgumentOutOfRangeException(nameof(options));
        this.options = options;
        requests = Channel.CreateBounded<Request>(new BoundedChannelOptions(options.MaxPendingRequests) { SingleReader = true, FullMode = BoundedChannelFullMode.Wait, AllowSynchronousContinuations = false });
        new Thread(Dispatch) { IsBackground = true, Name = "EmbedPDF dispatcher" }.Start();
    }
    public static async Task<EngineDispatcher> CreateAsync(PdfEngineOptions? options = null)
    {
        var engine = new EngineDispatcher(options ?? new()); await engine.ready.Task.ConfigureAwait(false); return engine;
    }
    internal Task<WireResult> Enqueue(ulong document, uint operation, object args, IReadOnlyList<ReadOnlyMemory<byte>>? payloads, CancellationToken cancellation)
    {
        var completion = new TaskCompletionSource<WireResult>(TaskCreationOptions.RunContinuationsAsynchronously);
        if (cancellation.IsCancellationRequested) { completion.SetCanceled(cancellation); return completion.Task; }
        // Admission precedes payload copies. Saturated callers hold no dispatcher work or owned input.
        var json = Wire.Encode(args); long bytes = json.LongLength;
        bytes = checked(bytes + 4); if (payloads != null) foreach (var payload in payloads) bytes = checked(bytes + 4 + payload.Length);
        lock (gate)
        {
            if (closing) { completion.SetException(new ObjectDisposedException(nameof(PdfEngine))); return completion.Task; }
            if (pending >= options.MaxPendingRequests || bytes > options.MaxQueuedBytes - queuedBytes)
            { completion.SetException(new PdfException("Busy", "Managed request slot or byte budget is exhausted", 5)); return completion.Task; }
            var frame = Wire.PayloadFrame(payloads ?? []);
            pending++; queuedBytes += bytes;
            if (!requests.Writer.TryWrite(new(sequence++, document, operation, json, frame, bytes, cancellation, completion)))
            { pending--; queuedBytes -= bytes; completion.SetException(new ObjectDisposedException(nameof(PdfEngine))); }
            wake.Set();
        }
        return completion.Task;
    }
    internal Task CloseDocument(ulong document)
    {
        lock (gate)
        {
            if (closing) return stopped.Task;
            var completion = new TaskCompletionSource<WireResult>(TaskCreationOptions.RunContinuationsAsynchronously);
            closes.Enqueue(new(sequence++, document, 2, Wire.Encode(new { }), Wire.PayloadFrame([]), 0, default, completion));
            wake.Set(); return completion.Task;
        }
    }
    Request? Next()
    {
        while (true)
        {
            lock (gate)
            {
                var ordinary = requests.Reader.TryPeek(out var request);
                if (closes.TryPeek(out var close) && (!ordinary || close.Sequence < request!.Sequence)) return closes.Dequeue();
                if (ordinary && requests.Reader.TryRead(out request)) return request;
                if (closing) return null;
            }
            wake.WaitOne();
        }
    }
    internal bool IsClosing { get { lock (gate) return closing; } }
    public ValueTask DisposeAsync()
    {
        lock (gate) { if (!closing) { closing = true; requests.Writer.TryComplete(); wake.Set(); } }
        return new(stopped.Task);
    }
    unsafe void Initialize()
    {
        if (NativeMethods.AbiVersion() >> 16 != 1) throw new PdfException("AbiMismatch", "EmbedPDF native ABI major must be 1");
        var fonts = options.StartupFonts.IsDefault ? [] : options.StartupFonts;
        var json = Wire.Encode(new { options.Limits, startupFonts = fonts.Select((font, index) => new { font.Key, font.FamilyName, font.Weight, font.Italic, font.Fallback, payloadIndex = index }).ToArray() });
        var payloads = Wire.PayloadFrame(fonts.Select(font => font.Bytes).ToArray());
        uint status; ulong id, error;
        fixed (byte* a = json, p = payloads) status = NativeMethods.Create(a, (ulong)json.Length, p, (ulong)payloads.Length, out id, out error);
        if (id != 0) handle = new(id);
        if (error != 0) Wire.CopyResult(error, status);
        Wire.Status(status);
        if (handle == null) throw new PdfException("InvalidHandle", "Native create returned no engine");
    }
    void Dispatch()
    {
        Exception? failure = null;
        try
        {
            Initialize(); ready.TrySetResult();
            while (Next() is { } request)
            {
                WireResult? value = null; Exception? refusal = null; bool canceled = false;
                try
                {
                    request.Cancellation.ThrowIfCancellationRequested();
                    var result = Execute(request.Document, request.Operation, request.Args, request.Payloads, request.Cancellation);
                    if (request.Cancellation.IsCancellationRequested)
                    {
                        if (result.Document != 0) Execute(result.Document, 2, Wire.Encode(new { }), Wire.PayloadFrame([]), default);
                        request.Cancellation.ThrowIfCancellationRequested();
                    }
                    value = result;
                }
                catch (OperationCanceledException) { canceled = true; }
                catch (Exception error) { refusal = error; }
                finally { if (request.Bytes != 0) lock (gate) { pending--; queuedBytes -= request.Bytes; } }
                if (canceled) request.Completion.TrySetCanceled(request.Cancellation.IsCancellationRequested ? request.Cancellation : new CancellationToken(true));
                else if (refusal != null) request.Completion.TrySetException(refusal);
                else request.Completion.TrySetResult(value!);
            }
        }
        catch (Exception error) { failure = error; ready.TrySetException(error); }
        finally
        {
            lock (gate) { closing = true; requests.Writer.TryComplete(); }
            while (requests.Reader.TryRead(out var request)) { request.Completion.TrySetException(failure ?? new ObjectDisposedException(nameof(PdfEngine))); lock (gate) { pending--; queuedBytes -= request.Bytes; } }
            while (closes.TryDequeue(out var close)) close.Completion.TrySetException(failure ?? new ObjectDisposedException(nameof(PdfEngine)));
            try { if (handle != null) { using var lease = handle.Acquire(); Wire.Status(NativeMethods.Close(lease.Value, uint.MaxValue)); } }
            catch (Exception error) { failure ??= error; }
            finally { handle?.Dispose(); }
            wake.Dispose();
            if (failure == null) stopped.TrySetResult(); else stopped.TrySetException(failure);
        }
    }
    unsafe WireResult Execute(ulong document, uint operation, byte[] args, byte[] payloads, CancellationToken cancellation)
    {
        using var engine = handle!.Acquire(); uint status; ulong jobId, errorId;
        fixed (byte* a = args, p = payloads) status = NativeMethods.Submit(engine.Value, document, operation, a, (ulong)args.Length, p, (ulong)payloads.Length, out jobId, out errorId);
        using var job = jobId == 0 ? null : new JobHandle(jobId);
        if (errorId != 0) Wire.CopyResult(errorId, status); Wire.Status(status);
        if (job == null) throw new PdfException("InvalidHandle", "Native submit returned no job");
        using var registration = cancellation.UnsafeRegister(static state => ((JobHandle)state!).Cancel(), job);
        using var lease = job.Acquire(); status = NativeMethods.Wait(lease.Value, uint.MaxValue, out var reply);
        var result = Wire.CopyResult(reply.Result, status);
        return result with { Document = reply.Document };
    }
}

public sealed class PdfEngine : IAsyncDisposable
{
    readonly EngineDispatcher dispatcher;
    PdfEngine(EngineDispatcher dispatcher) { this.dispatcher = dispatcher; }
    ~PdfEngine() { _ = dispatcher.DisposeAsync(); }
    public static async Task<PdfEngine> CreateAsync(PdfEngineOptions? options = null)
        => new(await EngineDispatcher.CreateAsync(options).ConfigureAwait(false));
    public async Task<PdfDocument> OpenAsync(ReadOnlyMemory<byte> bytes, string? password = null, CancellationToken cancellationToken = default)
    {
        var result = await dispatcher.Enqueue(0, 1, new { password }, [bytes], cancellationToken).ConfigureAwait(false);
        if (result.Document == 0) throw new PdfException("InvalidHandle", "Successful open returned no document");
        return new(this, result.Document);
    }
    internal Task<WireResult> Enqueue(ulong document, uint operation, object args, IReadOnlyList<ReadOnlyMemory<byte>>? payloads, CancellationToken cancellation)
        => dispatcher.Enqueue(document, operation, args, payloads, cancellation);
    internal Task CloseDocument(ulong document) => dispatcher.CloseDocument(document);
    public int PendingRequests => dispatcher.PendingRequests;
    public long QueuedBytes => dispatcher.QueuedBytes;
    public ValueTask DisposeAsync() { GC.SuppressFinalize(this); return dispatcher.DisposeAsync(); }
}
