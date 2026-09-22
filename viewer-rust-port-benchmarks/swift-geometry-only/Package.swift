// swift-tools-version: 6.4
import PackageDescription

let exports = [
    "swift_alloc_doubles", "swift_free_doubles", "swift_normalize_deg",
    "swift_rect_quad", "swift_quad_scale_about", "swift_quad_rotate_about",
    "swift_quad_bounds", "swift_point_in_quad",
]

let package = Package(
    name: "SwiftGeometry",
    products: [.executable(name: "SwiftGeometry", targets: ["SwiftGeometry"])],
    targets: [
        .executableTarget(
            name: "SwiftGeometry",
            linkerSettings: [.unsafeFlags(exports.flatMap { ["-Xlinker", "--export=\($0)"] })]
        )
    ]
)
