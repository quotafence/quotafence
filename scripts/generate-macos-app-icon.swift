import AppKit
import Foundation

let fileManager = FileManager.default
let projectRoot = URL(fileURLWithPath: fileManager.currentDirectoryPath)
let canvasSize = 1024

// NSApplication.setApplicationIconImage displays the full bitmap bounds in the
// Dock. Native asset-catalog icons receive an additional system enclosure inset,
// so runtime PNGs need an equivalent transparent safe area to match other apps.
let dockSafeAreaScale = 0.82

let iconTool = URL(
    fileURLWithPath:
        "/Applications/Xcode.app/Contents/Applications/Icon Composer.app/Contents/Executables/ictool"
)

func exportRenderedIcon(packageName: String, to outputURL: URL) throws {
    let packageURL = projectRoot.appendingPathComponent("logo/\(packageName)")
    let process = Process()
    process.executableURL = iconTool
    process.arguments = [
        packageURL.path,
        "--export-image",
        "--output-file", outputURL.path,
        "--platform", "macOS",
        "--rendition", "Default",
        "--width", String(canvasSize),
        "--height", String(canvasSize),
        "--scale", "1",
    ]
    try process.run()
    process.waitUntilExit()
    guard process.terminationStatus == 0 else {
        throw NSError(domain: "QuotaFenceIcon", code: Int(process.terminationStatus), userInfo: [
            NSLocalizedDescriptionKey: "Icon Composer could not render \(packageName)."
        ])
    }
}

func writeDockSizedIcon(renderedURL: URL, outputName: String) throws {
    guard let rendered = NSImage(contentsOf: renderedURL) else {
        throw NSError(domain: "QuotaFenceIcon", code: 1, userInfo: [
            NSLocalizedDescriptionKey: "Could not read rendered icon at \(renderedURL.path)."
        ])
    }
    guard let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil,
        pixelsWide: canvasSize,
        pixelsHigh: canvasSize,
        bitsPerSample: 8,
        samplesPerPixel: 4,
        hasAlpha: true,
        isPlanar: false,
        colorSpaceName: .deviceRGB,
        bytesPerRow: 0,
        bitsPerPixel: 0
    ), let context = NSGraphicsContext(bitmapImageRep: bitmap) else {
        throw NSError(domain: "QuotaFenceIcon", code: 2, userInfo: [
            NSLocalizedDescriptionKey: "Could not create the app-icon canvas."
        ])
    }

    let renderedSize = CGFloat(canvasSize) * dockSafeAreaScale
    let inset = (CGFloat(canvasSize) - renderedSize) / 2

    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    context.imageInterpolation = .high
    NSColor.clear.setFill()
    NSRect(x: 0, y: 0, width: canvasSize, height: canvasSize).fill()
    rendered.draw(
        in: NSRect(x: inset, y: inset, width: renderedSize, height: renderedSize),
        from: NSRect(origin: .zero, size: rendered.size),
        operation: .sourceOver,
        fraction: 1,
        respectFlipped: true,
        hints: [.interpolation: NSImageInterpolation.high]
    )
    context.flushGraphics()
    NSGraphicsContext.restoreGraphicsState()

    guard let png = bitmap.representation(using: .png, properties: [:]) else {
        throw NSError(domain: "QuotaFenceIcon", code: 3, userInfo: [
            NSLocalizedDescriptionKey: "Could not encode the app icon."
        ])
    }
    let outputURL = projectRoot.appendingPathComponent("logo/\(outputName)")
    try png.write(to: outputURL, options: .atomic)
    print(outputURL.path)
}

func generate(packageName: String, outputName: String) throws {
    let temporaryURL = fileManager.temporaryDirectory
        .appendingPathComponent("quotafence-\(UUID().uuidString).png")
    defer { try? fileManager.removeItem(at: temporaryURL) }
    try exportRenderedIcon(packageName: packageName, to: temporaryURL)
    try writeDockSizedIcon(renderedURL: temporaryURL, outputName: outputName)
}

try generate(packageName: "QuotaFence.icon", outputName: "quotafence-app-icon.png")
try generate(packageName: "QuotaFence-Light.icon", outputName: "quotafence-app-icon-light.png")
