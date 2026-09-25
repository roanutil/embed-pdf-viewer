// usage: swift run Shell <file.pdf> [out.png]
//
// A consumer of the EmbedPDFEngine facade. It imports one module and sees no
// Crux, no bincode, no blob ids and no wire JSON.
import EmbedPDFEngine
import Foundation
import ImageIO
import UniformTypeIdentifiers

let arguments = CommandLine.arguments
guard arguments.count >= 2 else {
    FileHandle.standardError.write("usage: Shell <file.pdf> [out.png]\n".data(using: .utf8)!)
    exit(2)
}
let data = try Data(contentsOf: URL(fileURLWithPath: arguments[1]))
let output = arguments.count > 2 ? arguments[2] : "page.png"

func milliseconds(since start: Date) -> Int { Int(Date().timeIntervalSince(start) * 1000) }

let engine = PDFEngine()

var started = Date()
let document = try await engine.open(data: data)
print("opened \(document.pageCount) page(s) in \(milliseconds(since: started)) ms")
for page in document.pages.prefix(3) {
    print("  page \(page.index): object \(page.objectNumber), \(page.size.width) x \(page.size.height) pt")
}

let page = document.page(at: 0)
started = Date()
let image = try await page.render(scale: 2)
print("rendered \(image.width) x \(image.height) in \(milliseconds(since: started)) ms")

let destination = CGImageDestinationCreateWithURL(
    URL(fileURLWithPath: output) as CFURL, UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(destination, image, nil)
CGImageDestinationFinalize(destination)
print("wrote \(output)")

let text = try await page.text()
print("text: \(text.count) characters: \(text.prefix(60).debugDescription)")
