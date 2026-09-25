// The Crux shell loop, owned by the SDK so that no consumer writes one.
//
// Everything Crux-shaped stops here: bincode, the effect switch, blob ids and
// the view model. Above this file the API is async methods and typed errors.
import EmbedPdfCrux
import EpdfCruxCore
import Foundation

/// One Crux core and the requests waiting on it.
///
/// Thread-safe by a lock rather than an actor, because Crux calls back from
/// the PDFium owner thread and an actor hop there would be a second queue for
/// no gain. Nothing is ever awaited while the lock is held.
final class CoreLoop: @unchecked Sendable {
    private let lock = NSLock()
    private var ffi: CoreFfi!
    private var nextRequest: UInt32 = 0
    private var requests: [UInt32: CheckedContinuation<ShellEffect, Error>] = [:]
    private var opens: [String: CheckedContinuation<DocumentView, Never>] = [:]

    /// Effects arrive here, off the owner thread, in the order Crux sent them.
    fileprivate let queue = DispatchQueue(label: "com.embedpdf.crux.effects")

    init() {
        ffi = CoreFfi(shell: WeakShell(self))
    }

    // MARK: Blobs

    func put(_ data: Data) -> Blob {
        Blob(id: ffi.putBlob(bytes: data))
    }

    func take(_ blob: Blob) -> Data? {
        ffi.takeBlob(id: blob.id)
    }

    // MARK: Requests

    /// Sends the event `make` builds for a fresh request number, and returns
    /// the one effect that answers it.
    ///
    /// Cancellation stops the wait, not the work. Every operation behind this
    /// is a single non-preemptible engine call, so a cancelled render still
    /// renders, and its pixels are taken and dropped when they arrive.
    func request(_ make: (UInt32) -> Event) async throws -> ShellEffect {
        let id: UInt32 = lock.withLock {
            nextRequest &+= 1
            return nextRequest
        }
        let event = make(id)
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                // Registered before sending: the answer can arrive on another
                // thread before `send` has even returned.
                lock.withLock { requests[id] = continuation }
                if Task.isCancelled {
                    cancel(id)
                    return
                }
                do {
                    try send(event)
                } catch {
                    lock.withLock { requests.removeValue(forKey: id) }?.resume(throwing: error)
                }
            }
        } onCancel: {
            cancel(id)
        }
    }

    /// Opens a document and waits until its view leaves `opening`.
    ///
    /// Not cancellable: an open that stopped waiting would leave a session
    /// behind under an id nobody holds.
    func open(docId: String, document: Blob, password: String?) async -> DocumentView {
        await withCheckedContinuation { continuation in
            lock.withLock { opens[docId] = continuation }
            do {
                try send(.open(docId: docId, document: document, password: password))
            } catch {
                // The FFI refused the event itself; report it as the view
                // would have, so the caller has one path to handle.
                lock.withLock { opens.removeValue(forKey: docId) }?.resume(
                    returning: DocumentView(
                        docId: docId,
                        state: .failed(code: "WireFormat", message: "\(error)"),
                        pages: []))
            }
        }
    }

    /// Fire and forget: nothing waits on a close.
    func send(_ event: Event) throws {
        let effects = try ffi.update(event: Data(try event.bincodeSerialize()))
        queue.async { self.handle(effects) }
    }

    private func cancel(_ id: UInt32) {
        lock.withLock { requests.removeValue(forKey: id) }?.resume(throwing: CancellationError())
    }

    // MARK: Effects

    fileprivate func handle(_ bytes: Data) {
        guard let batch = try? Requests.bincodeDeserialize(input: [UInt8](bytes)) else {
            // Our own types, generated from our own Rust. A decode failure is
            // a build mismatch between the two halves, not a runtime state.
            preconditionFailure("the core and the generated Swift disagree")
        }
        for request in batch.value {
            switch request.effect {
            case .render:
                resumeOpens()
            case .bitmap(let bitmap):
                resume(bitmap.request, with: request.effect, orDrop: [bitmap.rgba])
            case .text(let text):
                resume(text.request, with: request.effect)
            case .reply(let reply):
                resume(reply.request, with: request.effect, orDrop: reply.payloads)
            case .failed(let failed):
                resume(failed.request, with: request.effect)
            }
        }
    }

    /// Hands an answer to its caller, or, for one that was cancelled, takes
    /// its blobs so they do not stay in the store for the life of the engine.
    private func resume(_ id: UInt32, with effect: ShellEffect, orDrop blobs: [Blob] = []) {
        if let waiting = lock.withLock({ requests.removeValue(forKey: id) }) {
            waiting.resume(returning: effect)
        } else {
            for blob in blobs { _ = take(blob) }
        }
    }

    /// Crux's answer to an open is a redraw, not a reply, so every redraw
    /// re-reads the view and releases the opens that have settled.
    private func resumeOpens() {
        let waiting = lock.withLock { !opens.isEmpty }
        guard waiting, let view = try? ViewModel.bincodeDeserialize(input: [UInt8](ffi.view()))
        else { return }
        for document in view.documents where document.state != .opening {
            lock.withLock { opens.removeValue(forKey: document.docId) }?.resume(returning: document)
        }
    }
}

/// The shell Crux holds. Weak, because the core holds Crux: a strong reference
/// here would keep both alive forever, and the owner thread with them.
///
/// The upgrade happens on the effect queue rather than on the owner thread.
/// If the last reference to the loop went away while an effect was in
/// flight, the core would otherwise be released on the owner thread, and
/// dropping it joins that same thread.
private final class WeakShell: CruxShell, @unchecked Sendable {
    private weak var loop: CoreLoop?
    private let queue: DispatchQueue

    init(_ loop: CoreLoop) {
        self.loop = loop
        self.queue = loop.queue
    }

    func processEffects(bytes: Data) {
        queue.async { [weak loop] in loop?.handle(bytes) }
    }
}
