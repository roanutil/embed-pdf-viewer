import CPDFium

#if os(WASI)
import WASILibc
#else
import Darwin
#endif

private let ascentFlip: UInt32 = UInt32(EPDF_CHARGEO_SYNTHESIZED) << 1
private let recordBytes = 112

@inline(__always)
private func put<T>(_ value: T, _ base: UnsafeMutableRawPointer, _ offset: Int, _: T.Type) {
    base.advanced(by: offset).storeBytes(of: value, as: T.self)
}

@inline(__always)
private func putRect(_ value: FS_RECTF, _ base: UnsafeMutableRawPointer, _ offset: Int) {
    put(value.left, base, offset, Float.self)
    put(value.top, base, offset + 4, Float.self)
    put(value.right, base, offset + 8, Float.self)
    put(value.bottom, base, offset + 12, Float.self)
}

@inline(__always)
private func putQuad(_ value: FS_QUADPOINTSF, _ base: UnsafeMutableRawPointer, _ offset: Int) {
    put(value.x1, base, offset, Float.self)
    put(value.y1, base, offset + 4, Float.self)
    put(value.x2, base, offset + 8, Float.self)
    put(value.y2, base, offset + 12, Float.self)
    put(value.x3, base, offset + 16, Float.self)
    put(value.y3, base, offset + 20, Float.self)
    put(value.x4, base, offset + 24, Float.self)
    put(value.y4, base, offset + 28, Float.self)
}

@_cdecl("sw_read_page_geometry")
public func readPageGeometry(
    _ rawTextPage: UnsafeMutableRawPointer?,
    _ output: UnsafeMutableRawPointer?,
    _ capacity: Int32
) -> Int32 {
    let textPage = OpaquePointer(rawTextPage)
    let count = max(FPDFText_CountChars(textPage), 0)
    guard let output, capacity > 0 else { return count }

    var previous: OpaquePointer?
    var havePrevious = false
    var geometry = EPDF_CHAR_GEOMETRY()

    for index in 0..<count {
        let slot = output.advanced(by: Int(index) * recordBytes)
        if index < capacity {
            slot.initializeMemory(as: UInt8.self, repeating: 0, count: recordBytes)
        }
        let object = FPDFText_GetTextObject(textPage, index)
        let ok = EPDFText_GetCharGeometry(textPage, index, &geometry) != 0
        guard index < capacity else { continue }

        let objectKey = UInt32(truncatingIfNeeded: UInt(bitPattern: object))
        put(objectKey, slot, 4, UInt32.self)
        if !havePrevious || previous != object {
            put(Float(FPDFText_GetFontSize(textPage, index)), slot, 8, Float.self)
        }
        previous = object
        havePrevious = true

        let nativeFlags = UInt32(geometry.flags)
        if !ok || (nativeFlags & UInt32(EPDF_CHARGEO_EMPTY)) != 0 {
            put(UInt32(EPDF_CHARGEO_EMPTY | EPDF_CHARGEO_UPRIGHT), slot, 0, UInt32.self)
            continue
        }

        var flags = nativeFlags
        putRect(geometry.loose_box, slot, 16)
        if (flags & UInt32(EPDF_CHARGEO_HAS_TIGHT_BOX)) != 0 {
            putRect(geometry.tight_box, slot, 32)
        }
        if (flags & UInt32(EPDF_CHARGEO_HAS_LOOSE_QUAD)) != 0 {
            putQuad(geometry.loose_quad, slot, 48)
            if (flags & UInt32(EPDF_CHARGEO_HAS_TIGHT_QUAD)) != 0 {
                putQuad(geometry.tight_quad, slot, 80)
            }
            if (flags & UInt32(EPDF_CHARGEO_UPRIGHT)) == 0 {
                let matrix = geometry.matrix
                put(Float(atan2(Double(matrix.b), Double(matrix.a))), slot, 12, Float.self)
                if Double(matrix.a) * Double(matrix.d) - Double(matrix.b) * Double(matrix.c) < 0 {
                    flags |= ascentFlip
                }
            }
        }
        put(flags, slot, 0, UInt32.self)
    }
    return count > capacity ? -count : count
}

@_cdecl("sw_read_page_text")
public func readPageText(
    _ rawTextPage: UnsafeMutableRawPointer?,
    _ buffer: UnsafeMutablePointer<UInt16>?,
    _ length: Int32
) -> Int32 {
    EPDFText_GetTextFull(OpaquePointer(rawTextPage), buffer, length)
}

@_cdecl("sw_render_page")
public func renderPage(
    _ rawPage: UnsafeMutableRawPointer?,
    _ rawBitmap: UnsafeMutableRawPointer?,
    _ width: Int32,
    _ height: Int32,
    _ rotation: Int32
) -> Int32 {
    let bitmap = OpaquePointer(rawBitmap)
    _ = FPDFBitmap_FillRect(bitmap, 0, 0, width, height, 0xffff_ffff)
    FPDF_RenderPageBitmap(bitmap, OpaquePointer(rawPage), 0, 0, width, height, rotation, 0)
    return 1
}

@_cdecl("sw_search_page")
public func searchPage(
    _ rawTextPage: UnsafeMutableRawPointer?,
    _ term: UnsafePointer<UInt16>?,
    _ output: UnsafeMutablePointer<Float>?,
    _ capacity: Int32
) -> Int32 {
    let textPage = OpaquePointer(rawTextPage)
    guard let search = FPDFText_FindStart(textPage, term, 0, 0) else { return -1 }
    defer { FPDFText_FindClose(search) }
    let probe = output == nil || capacity <= 0
    var written: Int32 = 0
    while FPDFText_FindNext(search) != 0 {
        let start = FPDFText_GetSchResultIndex(search)
        let count = FPDFText_GetSchCount(search)
        let rectangles = FPDFText_CountRects(textPage, start, count)
        for rectangle in 0..<rectangles {
            var left = 0.0
            var top = 0.0
            var right = 0.0
            var bottom = 0.0
            _ = FPDFText_GetRect(textPage, rectangle, &left, &top, &right, &bottom)
            if !probe, written < capacity, let output {
                let slot = output.advanced(by: Int(written) * 4)
                slot[0] = Float(left)
                slot[1] = Float(top)
                slot[2] = Float(right)
                slot[3] = Float(bottom)
            }
            written += 1
        }
    }
    return !probe && written > capacity ? -written : written
}
