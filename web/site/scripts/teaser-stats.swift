// Replace the v12 teaser's 36–42s evidence card using the same JSON as the website.
// The original picture outside that interval and the complete original audio are retained.
// macOS only; compile with `swiftc -parse-as-library -O teaser-stats.swift -o teaser-stats`.
import Foundation
import AppKit
import AVFoundation
import CryptoKit

struct Evidence: Decodable {
    struct Study: Decodable {
        let repository: String, model: String
        let tasks: [String]
        let repetitions: Int
    }
    struct Mode: Decodable {
        let fewer_requests_percent: Int
        let mean_requests: Double, mean_requests_alone: Double
        let hidden_test_passes: Int, hidden_test_passes_alone: Int, of: Int
        let quality_rule_passed: Bool?
    }
    struct Result: Decodable {
        struct Modes: Decodable { let advisory: Mode, execution: Mode }
        let valid: Bool
        let modes: Modes
    }
    let status: String, study: Study, result: Result
}

struct ModEvidence: Decodable {
    struct Study: Decodable { let repository: String, model: String; let tasks: Int, repetitions: Int, run: Int }
    struct Arm: Decodable { let hidden_passed: Int, of: Int }
    struct Arms: Decodable { let a0: Arm, mod: Arm }
    struct Comparison: Decodable { let fewer_requests_percent: Double }
    let study: Study, arms: Arms, mod_vs_alone: Comparison
}

struct RenderError: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

let width = 1920, height = 1080
let cardStart = 36.0, cardEnd = 42.0

struct Palette {
    let background: NSColor, foreground: NSColor, muted: NSColor, accent: NSColor, green: NSColor
    init(day: Bool) {
        background = day ? NSColor(srgbRed: 0.92549, green: 0.92549, blue: 0.93725, alpha: 1) : NSColor(srgbRed: 0.025, green: 0.031, blue: 0.055, alpha: 1)
        foreground = day ? NSColor(srgbRed: 0.09, green: 0.10, blue: 0.15, alpha: 1) : NSColor(srgbRed: 0.96, green: 0.96, blue: 1, alpha: 1)
        muted = day ? NSColor(srgbRed: 0.35, green: 0.37, blue: 0.44, alpha: 1) : NSColor(srgbRed: 0.55, green: 0.58, blue: 0.67, alpha: 1)
        accent = day ? NSColor(srgbRed: 0.40, green: 0.31, blue: 0.71, alpha: 1) : NSColor(srgbRed: 0.60, green: 0.54, blue: 1, alpha: 1)
        green = day ? NSColor(srgbRed: 0.075, green: 0.475, blue: 0.39, alpha: 1) : NSColor(srgbRed: 0.38, green: 0.94, blue: 0.72, alpha: 1)
    }
}

func text(_ value: String, x: Double, y: Double, size: Double, color: NSColor,
          weight: NSFont.Weight = .regular, mono: Bool = false, center: Bool = false) {
    let font = mono ? NSFont.monospacedSystemFont(ofSize: size, weight: weight) : NSFont.systemFont(ofSize: size, weight: weight)
    let attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: color, .kern: size > 45 ? -size * 0.035 : 0]
    let string = value as NSString
    string.draw(at: NSPoint(x: center ? x - string.size(withAttributes: attributes).width / 2 : x, y: y), withAttributes: attributes)
}

func number(_ value: Double) -> String { value == value.rounded() ? String(Int(value)) : String(format: "%.2f", value) }

func card(_ time: Double, evidence: Evidence, mod: ModEvidence, palette p: Palette) throws -> CVPixelBuffer {
    var pixelBuffer: CVPixelBuffer?
    let attributes: [String: Any] = [kCVPixelBufferCGImageCompatibilityKey as String: true, kCVPixelBufferCGBitmapContextCompatibilityKey as String: true]
    let status = CVPixelBufferCreate(kCFAllocatorDefault, width, height, kCVPixelFormatType_32ARGB, attributes as CFDictionary, &pixelBuffer)
    guard status == kCVReturnSuccess, let buffer = pixelBuffer else { throw RenderError("Cannot allocate evidence frame: \(status)") }
    CVPixelBufferLockBaseAddress(buffer, [])
    defer { CVPixelBufferUnlockBaseAddress(buffer, []) }
    guard let context = CGContext(data: CVPixelBufferGetBaseAddress(buffer), width: width, height: height, bitsPerComponent: 8,
                                  bytesPerRow: CVPixelBufferGetBytesPerRow(buffer), space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipFirst.rawValue)
    else { throw RenderError("Cannot create evidence drawing context") }
    context.translateBy(x: 0, y: CGFloat(height)); context.scaleBy(x: 1.5, y: -1.5)
    let previous = NSGraphicsContext.current
    NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: true)
    defer { NSGraphicsContext.current = previous }
    context.setFillColor(p.background.cgColor); context.fill(CGRect(x: 0, y: 0, width: 1280, height: 720))
    let mode = evidence.result.modes.advisory
    text("adage", x: 64, y: 33, size: 23, color: p.foreground.withAlphaComponent(0.65), weight: .medium)
    text("Fewer calls in two studies.", x: 640, y: 96, size: 53, color: p.foreground, weight: .bold, center: true)
    text("ADAGE ADVICE", x: 98, y: 205, size: 14, color: p.accent, weight: .semibold, mono: true)
    text("CLAUDE CODE MOD PROTOTYPE", x: 777, y: 205, size: 13, color: p.accent, weight: .semibold, mono: true)
    let progress = 1 - pow(1 - max(0, min(1, time / 0.55)), 3)
    text("\(Int((Double(mode.fewer_requests_percent) * progress).rounded()))%", x: 98, y: 252, size: 116, color: p.green, weight: .bold)
    text("fewer model requests", x: 98, y: 420, size: 25, color: p.foreground, weight: .medium)
    text("\(mode.hidden_test_passes)/\(mode.of) runs passed · both arms", x: 98, y: 463, size: 18, color: p.muted)
    context.setStrokeColor(p.foreground.withAlphaComponent(0.13).cgColor); context.setLineWidth(1)
    context.move(to: CGPoint(x: 661, y: 260)); context.addLine(to: CGPoint(x: 661, y: 505)); context.strokePath()
    text("\(Int((mod.mod_vs_alone.fewer_requests_percent * progress).rounded()))%", x: 777, y: 252, size: 116, color: p.green, weight: .bold)
    text("fewer model requests", x: 777, y: 420, size: 25, color: p.foreground, weight: .medium)
    text("\(mod.arms.mod.hidden_passed)/\(mod.arms.mod.of) runs passed · both arms", x: 777, y: 463, size: 18, color: p.muted)
    text("\(evidence.study.repository) · \(evidence.study.model.replacingOccurrences(of: "Claude ", with: ""))", x: 98, y: 526, size: 17, color: p.muted)
    text("\(evidence.study.tasks.count) tasks × \(evidence.study.repetitions) runs · report-only", x: 98, y: 557, size: 17, color: p.muted)
    text("\(mod.study.repository) sandbox · \(mod.study.model.replacingOccurrences(of: "Claude ", with: ""))", x: 777, y: 526, size: 17, color: p.muted)
    text("\(mod.study.tasks) tasks × \(mod.study.repetitions) runs · development run \(mod.study.run)", x: 777, y: 557, size: 16, color: p.muted)
    text("Each vs agent alone. Hidden-test passes. Full results: /evidence", x: 640, y: 624, size: 18, color: p.muted, center: true)
    return buffer
}

func audioHash(_ asset: AVAsset) async throws -> String {
    let tracks = try await asset.loadTracks(withMediaType: .audio)
    guard tracks.count == 1 else { throw RenderError("Expected exactly one original score track") }
    let reader = try AVAssetReader(asset: asset)
    let output = AVAssetReaderTrackOutput(track: tracks[0], outputSettings: nil)
    reader.add(output)
    guard reader.startReading() else { throw reader.error! }
    var hash = SHA256()
    while let sample = output.copyNextSampleBuffer() {
        // Empty timing markers carry no compressed audio bytes (for example an AAC edit boundary).
        guard let block = CMSampleBufferGetDataBuffer(sample) else {
            guard CMSampleBufferGetNumSamples(sample) == 0 else { throw RenderError("Nonempty audio sample has no data") }
            continue
        }
        var data = Data(count: CMBlockBufferGetDataLength(block))
        let result = data.withUnsafeMutableBytes { bytes in CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: bytes.count, destination: bytes.baseAddress!) }
        guard result == kCMBlockBufferNoErr else { throw RenderError("Cannot read audio sample: \(result)") }
        hash.update(data: data)
    }
    guard reader.status == .completed else { throw reader.error! }
    return hash.finalize().map { String(format: "%02x", $0) }.joined()
}

func render(source: URL, output: URL, evidence: Evidence, mod: ModEvidence, day: Bool) async throws {
    print("Reading \(source.lastPathComponent)…"); fflush(stdout)
    let asset = AVURLAsset(url: source)
    let tracks = try await asset.loadTracks(withMediaType: .video)
    guard tracks.count == 1 else { throw RenderError("Expected one video track: \(source.path)") }
    let track = tracks[0], duration = try await asset.load(.duration)
    let size = try await track.load(.naturalSize), fps = try await track.load(.nominalFrameRate)
    guard abs(CMTimeGetSeconds(duration) - 45) < 0.05, size == CGSize(width: width, height: height), fps == 24 else {
        throw RenderError("Input must be the 45s, 1920×1080, 24fps v12 teaser")
    }
    guard !FileManager.default.fileExists(atPath: output.path) else { throw RenderError("Output already exists; choose a new output directory: \(output.path)") }
    let picture = output.deletingPathExtension().appendingPathExtension("picture.mp4")
    defer { try? FileManager.default.removeItem(at: picture) }
    let reader = try AVAssetReader(asset: asset)
    let read = AVAssetReaderTrackOutput(track: track, outputSettings: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32ARGB])
    read.alwaysCopiesSampleData = false; reader.add(read)
    let writer = try AVAssetWriter(outputURL: picture, fileType: .mp4)
    let write = AVAssetWriterInput(mediaType: .video, outputSettings: [AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: width, AVVideoHeightKey: height,
        AVVideoCompressionPropertiesKey: [AVVideoAverageBitRateKey: 2800000, AVVideoProfileLevelKey: AVVideoProfileLevelH264HighAutoLevel, AVVideoMaxKeyFrameIntervalKey: 48]])
    write.expectsMediaDataInRealTime = false; writer.add(write)
    let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: write, sourcePixelBufferAttributes: nil)
    guard writer.startWriting(), reader.startReading() else { throw writer.error ?? reader.error! }
    print("Rendering \(output.lastPathComponent)…"); fflush(stdout)
    writer.startSession(atSourceTime: .zero)
    var frames = 0, replaced = 0
    while let sample = read.copyNextSampleBuffer() {
        while !write.isReadyForMoreMediaData {
            try await Task.sleep(nanoseconds: 2_000_000)
            if writer.status == .failed { throw writer.error! }
        }
        try autoreleasepool {
            let timestamp = CMSampleBufferGetPresentationTimeStamp(sample), seconds = CMTimeGetSeconds(timestamp)
            let buffer: CVPixelBuffer
            if seconds >= cardStart && seconds < cardEnd {
                buffer = try card(seconds - cardStart, evidence: evidence, mod: mod, palette: Palette(day: day)); replaced += 1
            } else {
                guard let original = CMSampleBufferGetImageBuffer(sample) else { throw RenderError("Input frame has no picture") }
                buffer = original
            }
            guard adaptor.append(buffer, withPresentationTime: timestamp) else { throw writer.error! }
        }
        frames += 1
        if frames % 240 == 0 { print("\(output.lastPathComponent): \(frames / 24)s / 45s"); fflush(stdout) }
    }
    guard reader.status == .completed else { throw reader.error! }
    write.markAsFinished(); await writer.finishWriting()
    guard writer.status == .completed else { throw writer.error! }
    guard frames == 1080, replaced == 144 else { throw RenderError("Unexpected frame counts: \(frames) total / \(replaced) replaced") }
    let encoded = AVURLAsset(url: picture), composition = AVMutableComposition()
    let range = CMTimeRange(start: .zero, duration: duration)
    try composition.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid)!.insertTimeRange(range, of: try await encoded.loadTracks(withMediaType: .video)[0], at: .zero)
    for audio in try await asset.loadTracks(withMediaType: .audio) {
        try composition.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid)!.insertTimeRange(range, of: audio, at: .zero)
    }
    let export = AVAssetExportSession(asset: composition, presetName: AVAssetExportPresetPassthrough)!
    export.shouldOptimizeForNetworkUse = true
    try await export.export(to: output, as: .mp4)
    let check = AVURLAsset(url: output), video = try await check.loadTracks(withMediaType: .video)[0]
    let sameAudio = try await audioHash(asset) == audioHash(check)
    guard sameAudio else { throw RenderError("Original soundtrack changed") }
    let generator = AVAssetImageGenerator(asset: check)
    generator.appliesPreferredTrackTransform = true; generator.maximumSize = CGSize(width: 1280, height: 720)
    generator.requestedTimeToleranceBefore = .zero; generator.requestedTimeToleranceAfter = .zero
    for time in [2.5, 18.5, 32.0, 35.95, 36.0, 36.6, 39.0, 41.95, 42.0, 43.5] {
        let frame = try await generator.image(at: CMTime(seconds: time, preferredTimescale: 600))
        let jpeg = NSBitmapImageRep(cgImage: frame.image).representation(using: .jpeg, properties: [.compressionFactor: 0.9])!
        try jpeg.write(to: output.deletingPathExtension().appendingPathExtension(String(format: "%05.2f.jpg", time)))
    }
    print("VERIFIED \(output.lastPathComponent): \(CMTimeGetSeconds(try await check.load(.duration)))s; \(try await video.load(.naturalSize)); \(try await video.load(.nominalFrameRate))fps; \(frames) frames; \(replaced) evidence frames; original compressed audio SHA256 matches")
}

@main struct TeaserStats {
    static func main() async throws {
        let arguments = CommandLine.arguments
        guard arguments.count == 6 else { throw RenderError("Usage: teaser-stats HELDOUT.json CLAUDE-MOD.json DARK-v12.mp4 DAY-v12.mp4 OUTPUT-DIR") }
        let evidenceData = try Data(contentsOf: URL(fileURLWithPath: arguments[1]))
        let evidence = try JSONDecoder().decode(Evidence.self, from: evidenceData)
        let modData = try Data(contentsOf: URL(fileURLWithPath: arguments[2]))
        let mod = try JSONDecoder().decode(ModEvidence.self, from: modData)
        let mode = evidence.result.modes.advisory
        guard evidence.status == "result", evidence.result.valid,
              mode.hidden_test_passes == mode.hidden_test_passes_alone,
              mode.fewer_requests_percent > 0, mode.fewer_requests_percent < 100,
              mod.arms.mod.hidden_passed == mod.arms.a0.hidden_passed,
              mod.arms.mod.of == mod.arms.a0.of,
              mod.mod_vs_alone.fewer_requests_percent > 0, mod.mod_vs_alone.fewer_requests_percent < 100 else {
            throw RenderError("The evidence has changed: review the card's wording before rendering")
        }
        let directory = URL(fileURLWithPath: arguments[5], isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        for (name, input, day) in [("dark", arguments[3], false), ("day", arguments[4], true)] {
            try await render(source: URL(fileURLWithPath: input), output: directory.appendingPathComponent("teaser-evidence-\(name).mp4"), evidence: evidence, mod: mod, day: day)
        }
        try evidenceData.write(to: directory.appendingPathComponent("heldout-source.json"))
        try modData.write(to: directory.appendingPathComponent("claude-mod-source.json"))
    }
}
