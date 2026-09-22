#if os(WASI)
import WASILibc
#endif

struct Point { var x: Double; var y: Double }

@inline(__always) func normalize(_ degrees: Double) -> Double {
    let first = degrees.truncatingRemainder(dividingBy: 360)
    return (first + 360).truncatingRemainder(dividingBy: 360)
}

@inline(__always) func rotate(_ point: Point, _ about: Point, _ degrees: Double) -> Point {
    if degrees == 0 { return point }
    let radians = degrees * Double.pi / 180
    let dx = point.x - about.x, dy = point.y - about.y
    return Point(x: about.x + dx * cos(radians) - dy * sin(radians), y: about.y + dx * sin(radians) + dy * cos(radians))
}

@inline(__always) func input(_ address: UInt32) -> UnsafePointer<Double> {
    UnsafePointer<Double>(bitPattern: UInt(address))!
}
@inline(__always) func output(_ address: UInt32) -> UnsafeMutablePointer<Double> {
    UnsafeMutablePointer<Double>(bitPattern: UInt(address))!
}

@_cdecl("swift_alloc_doubles")
func allocateDoubles(_ count: Int32) -> UInt32 {
    let pointer = UnsafeMutablePointer<Double>.allocate(capacity: Int(count))
    return UInt32(truncatingIfNeeded: UInt(bitPattern: pointer))
}

@_cdecl("swift_free_doubles")
func freeDoubles(_ address: UInt32, _ count: Int32) {
    output(address).deallocate()
}

@_cdecl("swift_normalize_deg")
func normalizeDegrees(_ degrees: Double) -> Double { normalize(degrees) }

@_cdecl("swift_rect_quad")
func rectQuad(_ x: Double, _ y: Double, _ width: Double, _ height: Double, _ rotation: Double, _ result: UInt32) {
    let centre = Point(x: x + width / 2, y: y + height / 2)
    let corners = [Point(x: x, y: y), Point(x: x + width, y: y), Point(x: x + width, y: y + height), Point(x: x, y: y + height)]
    let target = output(result)
    for index in 0..<4 {
        let point = normalize(rotation) == 0 ? corners[index] : rotate(corners[index], centre, rotation)
        target[index * 2] = point.x
        target[index * 2 + 1] = point.y
    }
}

@_cdecl("swift_quad_scale_about")
func scaleQuad(_ source: UInt32, _ ax: Double, _ ay: Double, _ scale: Double, _ result: UInt32) {
    let values = input(source), target = output(result)
    for index in 0..<4 {
        target[index * 2] = ax + (values[index * 2] - ax) * scale
        target[index * 2 + 1] = ay + (values[index * 2 + 1] - ay) * scale
    }
}

@_cdecl("swift_quad_rotate_about")
func rotateQuad(_ source: UInt32, _ ax: Double, _ ay: Double, _ degrees: Double, _ result: UInt32) {
    let values = input(source), target = output(result), about = Point(x: ax, y: ay)
    for index in 0..<4 {
        let point = rotate(Point(x: values[index * 2], y: values[index * 2 + 1]), about, degrees)
        target[index * 2] = point.x
        target[index * 2 + 1] = point.y
    }
}

@_cdecl("swift_quad_bounds")
func quadBounds(_ source: UInt32, _ result: UInt32) {
    let values = input(source), target = output(result)
    var minX = Double.infinity, minY = Double.infinity
    var maxX = -Double.infinity, maxY = -Double.infinity
    for index in 0..<4 {
        minX = min(minX, values[index * 2]); minY = min(minY, values[index * 2 + 1])
        maxX = max(maxX, values[index * 2]); maxY = max(maxY, values[index * 2 + 1])
    }
    target[0] = minX; target[1] = minY; target[2] = maxX - minX; target[3] = maxY - minY
}

@_cdecl("swift_point_in_quad")
func pointInQuad(_ x: Double, _ y: Double, _ source: UInt32) -> Int32 {
    let values = input(source)
    var sign: Int32 = 0
    for index in 0..<4 {
        let next = (index + 1) % 4
        let ax = values[index * 2], ay = values[index * 2 + 1]
        let bx = values[next * 2], by = values[next * 2 + 1]
        let cross = (bx - ax) * (y - ay) - (by - ay) * (x - ax)
        if cross == 0 { continue }
        let current: Int32 = cross > 0 ? 1 : -1
        if sign == 0 { sign = current } else if sign != current { return 0 }
    }
    return 1
}
