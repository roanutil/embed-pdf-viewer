import Foundation
import PdfiumSwift

let args = CommandLine.arguments
guard args.count >= 2 else {
    FileHandle.standardError.write(Data("usage: PdfiumSwiftBench <pdf> [iterations]\n".utf8))
    exit(2)
}
let iterations = args.count > 2 ? max(Int(args[2]) ?? 1, 1) : 20
let engine = PdfiumSwiftEngine()
let document = try engine.open(Data(contentsOf: URL(fileURLWithPath: args[1])))
let page: UInt32 = 4

func measure(_ body: () throws -> Int) rethrows -> [String: Any] {
    var checksum = 0
    let start = DispatchTime.now().uptimeNanoseconds
    for _ in 0..<iterations { checksum &+= try body() }
    let elapsed = DispatchTime.now().uptimeNanoseconds - start
    return ["nsPerIteration": Double(elapsed) / Double(iterations), "checksum": checksum]
}

let result: [String: Any] = [
    "language": "swift",
    "iterations": iterations,
    "pageCount": try measure { Int(try document.pageCount()) },
    "pageSize": try measure { Int(try document.pageSize(page).width) },
    "pageText": try measure { try document.pageText(page).utf16.count },
    "search": try measure { try document.search(page, query: "the", caseSensitive: false).count },
    "render": try measure { try document.renderPage(page, scale: 0.25).bgra.count },
]
let bytes = try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
FileHandle.standardOutput.write(bytes)
FileHandle.standardOutput.write(Data("\n".utf8))
document.close()
