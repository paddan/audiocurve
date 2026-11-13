// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "AudioCurveCore",
    platforms: [.macOS("27.0")],
    products: [.library(name: "AudioCurveCore", targets: ["AudioCurveCore"])],
    targets: [
        .target(name: "CAudioCurveDSP"),
        .target(name: "AudioCurveCore", dependencies: ["CAudioCurveDSP"]),
        .testTarget(name: "AudioCurveCoreTests", dependencies: ["AudioCurveCore", "CAudioCurveDSP"],
                    resources: [.copy("Fixtures")]),
    ]
)
