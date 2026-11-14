#!/usr/bin/env -S swift -suppress-warnings
// Before/after pictures for a PR body: one side-by-side PNG per scene found in both folders, before left,
// after right, each under a small label. With light/ and dark/ in both folders, each pair is done into
// <out>/light and <out>/dark.
// Usage: scripts/snapshot-compare <before-dir> <after-dir> <out-dir> [markers.json]
// markers.json numbers spots on the after picture, in its own pixels (as its PNG is), per scene name:
//   { "21-jump-palette": [ { "x": 1180, "y": 240, "n": 1 }, { "x": 900, "y": 610, "n": 2 } ] }
// A scene's markers apply to both appearances.
import AppKit

struct Marker: Decodable { let x: Double; let y: Double; let n: Int }

let arguments = CommandLine.arguments
guard arguments.count == 4 || arguments.count == 5 else {
    FileHandle.standardError.write("usage: scripts/snapshot-compare <before-dir> <after-dir> <out-dir> [markers.json]\n".data(using: .utf8)!)
    exit(2)
}
let (beforeRoot, afterRoot, outRoot) = (URL(fileURLWithPath: arguments[1]), URL(fileURLWithPath: arguments[2]), URL(fileURLWithPath: arguments[3]))
var markers: [String: [Marker]] = [:]
if arguments.count == 5 {
    do {
        markers = try JSONDecoder().decode([String: [Marker]].self, from: Data(contentsOf: URL(fileURLWithPath: arguments[4])))
    } catch {
        FileHandle.standardError.write("could not read the markers: \(error)\n".data(using: .utf8)!)
        exit(1)
    }
}

let fm = FileManager.default
let band: CGFloat = 56
let gap: CGFloat = 24
let canvasColor = NSColor(calibratedWhite: 0.93, alpha: 1)
let markerColor = NSColor(calibratedRed: 0.90, green: 0.28, blue: 0.30, alpha: 1)

func scenes(in folder: URL) -> Set<String> {
    let names = (try? fm.contentsOfDirectory(atPath: folder.path)) ?? []
    return Set(names.filter { $0.hasSuffix(".png") }.map { String($0.dropLast(4)) })
}

func bitmap(_ url: URL) -> NSBitmapImageRep? {
    guard let data = try? Data(contentsOf: url) else { return nil }
    return NSBitmapImageRep(data: data)
}

func draw(_ text: String, at point: NSPoint, size: CGFloat, color: NSColor, centered: Bool = false) {
    let attributes: [NSAttributedString.Key: Any] = [
        .font: NSFont.boldSystemFont(ofSize: size),
        .foregroundColor: color,
    ]
    let string = NSAttributedString(string: text, attributes: attributes)
    let measured = string.size()
    let origin = centered ? NSPoint(x: point.x - measured.width / 2, y: point.y - measured.height / 2) : point
    string.draw(at: origin)
}

func compare(_ scene: String, before: NSBitmapImageRep, after: NSBitmapImageRep, into out: URL) -> Bool {
    let (bw, bh) = (CGFloat(before.pixelsWide), CGFloat(before.pixelsHigh))
    let (aw, ah) = (CGFloat(after.pixelsWide), CGFloat(after.pixelsHigh))
    let width = bw + gap + aw
    let height = band + max(bh, ah)
    guard let canvas = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: Int(width), pixelsHigh: Int(height), bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
    else { return false }
    canvas.size = NSSize(width: width, height: height)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: canvas)
    canvasColor.setFill()
    NSRect(x: 0, y: 0, width: width, height: height).fill()
    // AppKit counts y from the bottom; pictures hang from the band at the top.
    before.draw(in: NSRect(x: 0, y: height - band - bh, width: bw, height: bh))
    after.draw(in: NSRect(x: bw + gap, y: height - band - ah, width: aw, height: ah))
    let labelColor = NSColor(calibratedWhite: 0.25, alpha: 1)
    draw("Before", at: NSPoint(x: 8, y: height - band + 14), size: 26, color: labelColor)
    draw("After", at: NSPoint(x: bw + gap + 8, y: height - band + 14), size: 26, color: labelColor)
    for marker in markers[scene] ?? [] {
        let center = NSPoint(x: bw + gap + marker.x, y: height - band - marker.y)
        let radius: CGFloat = 22
        let dot = NSBezierPath(ovalIn: NSRect(x: center.x - radius, y: center.y - radius, width: radius * 2, height: radius * 2))
        markerColor.setFill()
        dot.fill()
        NSColor.white.setStroke()
        dot.lineWidth = 4
        dot.stroke()
        draw(String(marker.n), at: center, size: 24, color: .white, centered: true)
    }
    NSGraphicsContext.restoreGraphicsState()
    guard let png = canvas.representation(using: .png, properties: [:]) else { return false }
    do {
        try png.write(to: out.appendingPathComponent("\(scene).png"))
        return true
    } catch {
        FileHandle.standardError.write("could not save \(scene): \(error)\n".data(using: .utf8)!)
        return false
    }
}

func comparePair(before: URL, after: URL, out: URL) -> Int {
    try? fm.createDirectory(at: out, withIntermediateDirectories: true)
    let (beforeScenes, afterScenes) = (scenes(in: before), scenes(in: after))
    for missing in beforeScenes.symmetricDifference(afterScenes).sorted() {
        print("skipped \(missing): only in \(beforeScenes.contains(missing) ? "before" : "after")")
    }
    var made = 0
    for scene in beforeScenes.intersection(afterScenes).sorted() {
        guard let b = bitmap(before.appendingPathComponent("\(scene).png")),
              let a = bitmap(after.appendingPathComponent("\(scene).png")) else {
            print("skipped \(scene): unreadable PNG")
            continue
        }
        if compare(scene, before: b, after: a, into: out) { made += 1 }
    }
    print("\(out.path): \(made) pictures")
    return made
}

let appearances = ["light", "dark"].filter {
    fm.fileExists(atPath: beforeRoot.appendingPathComponent($0).path) && fm.fileExists(atPath: afterRoot.appendingPathComponent($0).path)
}
if appearances.isEmpty {
    _ = comparePair(before: beforeRoot, after: afterRoot, out: outRoot)
} else {
    for appearance in appearances {
        _ = comparePair(before: beforeRoot.appendingPathComponent(appearance), after: afterRoot.appendingPathComponent(appearance), out: outRoot.appendingPathComponent(appearance))
    }
}
