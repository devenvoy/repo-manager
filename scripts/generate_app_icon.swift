import AppKit

func renderIcon(size: CGFloat) -> NSImage {
    let image = NSImage(size: NSSize(width: size, height: size))
    image.lockFocus()

    let context = NSGraphicsContext.current!.cgContext
    let rect = CGRect(x: 0, y: 0, width: size, height: size)

    // Corner radius for macOS squircle (approx 22.5% of size)
    let cornerRadius = size * 0.225
    let insetRect = rect.insetBy(dx: size * 0.05, dy: size * 0.05)
    let path = CGPath(roundedRect: insetRect, cornerWidth: cornerRadius, cornerHeight: cornerRadius, transform: nil)

    // Shadow
    context.saveGState()
    context.setShadow(offset: CGSize(width: 0, height: -size * 0.04), blur: size * 0.08, color: CGColor(red: 0.1, green: 0.15, blue: 0.4, alpha: 0.25))

    // Background Gradient: #4f68f8 to #263bd7
    let colorSpace = CGColorSpaceCreateDeviceRGB()
    let colors = [
        CGColor(red: 79/255.0, green: 104/255.0, blue: 248/255.0, alpha: 1.0),
        CGColor(red: 38/255.0, green: 59/255.0, blue: 215/255.0, alpha: 1.0)
    ] as CFArray
    let gradient = CGGradient(colorsSpace: colorSpace, colors: colors, locations: [0.0, 1.0])!

    context.addPath(path)
    context.clip()
    context.drawLinearGradient(gradient, start: CGPoint(x: insetRect.midX, y: insetRect.maxY), end: CGPoint(x: insetRect.midX, y: insetRect.minY), options: [])
    context.restoreGState()

    // Inner subtle glow border
    context.saveGState()
    context.addPath(path)
    context.setStrokeColor(CGColor(red: 1.0, green: 1.0, blue: 1.0, alpha: 0.2))
    context.setLineWidth(max(1.0, size * 0.015))
    context.strokePath()
    context.restoreGState()

    // Draw Release Desk Layer Glyphs (3 layered cards)
    context.saveGState()
    context.setStrokeColor(CGColor(red: 1.0, green: 1.0, blue: 1.0, alpha: 0.95))
    context.setLineWidth(max(1.5, size * 0.045))
    context.setLineCap(.round)
    context.setLineJoin(.round)

    let cx = size * 0.5
    let cy = size * 0.5
    let w = size * 0.28
    let h = size * 0.14

    // Top rhomboid/layer
    let layer1 = CGMutablePath()
    layer1.move(to: CGPoint(x: cx, y: cy + size * 0.18))
    layer1.addLine(to: CGPoint(x: cx + w, y: cy + size * 0.18 - h))
    layer1.addLine(to: CGPoint(x: cx, y: cy + size * 0.18 - h * 2))
    layer1.addLine(to: CGPoint(x: cx - w, y: cy + size * 0.18 - h))
    layer1.closeSubpath()
    context.setFillColor(CGColor(red: 1.0, green: 1.0, blue: 1.0, alpha: 0.18))
    context.addPath(layer1)
    context.fillPath()
    context.addPath(layer1)
    context.strokePath()

    // Middle chevron
    let layer2 = CGMutablePath()
    layer2.move(to: CGPoint(x: cx - w, y: cy - size * 0.04))
    layer2.addLine(to: CGPoint(x: cx, y: cy - size * 0.04 - h))
    layer2.addLine(to: CGPoint(x: cx + w, y: cy - size * 0.04))
    context.addPath(layer2)
    context.strokePath()

    // Bottom chevron
    let layer3 = CGMutablePath()
    layer3.move(to: CGPoint(x: cx - w, y: cy - size * 0.16))
    layer3.addLine(to: CGPoint(x: cx, y: cy - size * 0.16 - h))
    layer3.addLine(to: CGPoint(x: cx + w, y: cy - size * 0.16))
    context.addPath(layer3)
    context.strokePath()

    context.restoreGState()

    image.unlockFocus()
    return image
}

func savePNG(image: NSImage, path: String) {
    guard let tiff = image.tiffRepresentation,
          let bitmap = NSBitmapImageRep(data: tiff),
          let png = bitmap.representation(using: .png, properties: [:]) else {
        fatalError("Failed to convert image to PNG")
    }
    try! png.write(to: URL(fileURLWithPath: path))
}

let iconsetDir = "resources/AppIcon.iconset"
try? FileManager.default.removeItem(atPath: iconsetDir)
try! FileManager.default.createDirectory(atPath: iconsetDir, withIntermediateDirectories: true)

let sizes: [(String, CGFloat)] = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
]

for (name, size) in sizes {
    let img = renderIcon(size: size)
    savePNG(image: img, path: "\(iconsetDir)/\(name)")
}

print("Generated iconset successfully.")
