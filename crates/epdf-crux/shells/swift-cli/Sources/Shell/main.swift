// usage: swift run Shell <file.pdf> [out.png]
//
// Opens a PDF through the Crux core, prints the view model, renders the first
// page to a PNG, and reads the metadata through the untyped escape hatch.
// Nothing here writes a wire envelope or parses engine JSON except that last
// call, which is the point.
import CoreGraphics
import EmbedPdfCrux
import EpdfCruxCore
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// The shell side of Crux: handles the effects the core cannot.
///
/// `processEffects` arrives on the PDFium owner thread, so everything goes
/// through one serial queue, which is what a UI shell's main actor would be.
final class Shell: CruxShell, @unchecked Sendable {
    private let queue = DispatchQueue(label: "shell")
    private let changed = DispatchSemaphore(value: 0)
    private(set) var bitmaps: [PageBitmap] = []
    private(set) var replies: [CallReply] = []
    var core: CoreFfi!

    func processEffects(bytes: Data) {
        queue.async { self.handle(bytes) }
    }

    func send(_ event: Event) throws {
        let effects = try core.update(event: Data(try event.bincodeSerialize()))
        queue.async { self.handle(effects) }
    }

    func view() throws -> ViewModel {
        try ViewModel.bincodeDeserialize(input: [UInt8](try core.view()))
    }

    /// Blocks until `ready` holds, re-checking after every effect.
    func wait(until ready: () throws -> Bool) rethrows {
        while try !ready() {
            guard changed.wait(timeout: .now() + 20) == .success else {
                fatalError("the core went quiet")
            }
        }
    }

    private func handle(_ bytes: Data) {
        let decodeStarted = Date()
        let requests = try! Requests.bincodeDeserialize(input: [UInt8](bytes))
        if bytes.count > 1_000_000 {
            print("  decoded \(bytes.count) bytes of effects in \(Int(Date().timeIntervalSince(decodeStarted) * 1000)) ms")
        }
        for request in requests.value {
            switch request.effect {
            case .render: break
            case .bitmap(let bitmap): bitmaps.append(bitmap)
            case .reply(let reply): replies.append(reply)
            }
            changed.signal()
        }
    }
}

let arguments = CommandLine.arguments
guard arguments.count >= 2 else {
    FileHandle.standardError.write("usage: Shell <file.pdf> [out.png]\n".data(using: .utf8)!)
    exit(2)
}
let pdf = try Data(contentsOf: URL(fileURLWithPath: arguments[1]))
let output = arguments.count > 2 ? arguments[2] : "page.png"

let shell = Shell()
shell.core = CoreFfi(shell: shell)

let started = Date()
let document = Blob(id: shell.core.putBlob(bytes: pdf))
try shell.send(.open(docId: "doc", document: document, password: nil))
try shell.wait { try shell.view().documents.first.map { $0.state != .opening } ?? false }
let opened = try shell.view().documents[0]
print("state: \(opened.state)  pages: \(opened.pages.count)  opened in \(Int(Date().timeIntervalSince(started) * 1000)) ms")
for page in opened.pages.prefix(3) {
    print("  page \(page.index): object \(page.pageObjectNumber), \(page.width) x \(page.height) pt")
}
guard opened.state == .ready, let first = opened.pages.first else { exit(1) }

let renderStarted = Date()
try shell.send(.render(docId: "doc", pageObjectNumber: first.pageObjectNumber, scale: 2))
shell.wait { !shell.bitmaps.isEmpty }
let bitmap = shell.bitmaps[0]
let rgba = shell.core.takeBlob(id: bitmap.rgba.id)!
print("bitmap: \(bitmap.width) x \(bitmap.height), stride \(bitmap.stride), \(rgba.count) bytes in \(Int(Date().timeIntervalSince(renderStarted) * 1000)) ms")

let image = CGImage(
    width: Int(bitmap.width), height: Int(bitmap.height),
    bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: Int(bitmap.stride),
    space: CGColorSpaceCreateDeviceRGB(),
    bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
    provider: CGDataProvider(data: rgba as CFData)!,
    decode: nil, shouldInterpolate: false, intent: .defaultIntent)!
let destination = CGImageDestinationCreateWithURL(
    URL(fileURLWithPath: output) as CFURL, UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(destination, image, nil)
CGImageDestinationFinalize(destination)
print("wrote \(output)")

let request = #"{"kind":"metadata.read","docId":"doc"}"#
try shell.send(.call(job: 1, json: request, payloads: []))
shell.wait { !shell.replies.isEmpty }
print("metadata.read -> \(shell.replies[0].json.prefix(160))")
