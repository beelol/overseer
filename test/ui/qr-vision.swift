// Test-only: reads the QR codes in an image with the Mac's own reader (the Vision framework, the
// reader behind the Camera app), so a screenshot of the pairing panel is checked the way a phone
// would read it. Prints {"codes": ["…"]}.
// Build: swiftc -O test/ui/qr-vision.swift -o qr-vision     Run: qr-vision screenshot.png
import AppKit
import Foundation
import Vision

let args = CommandLine.arguments
guard args.count >= 2, let image = NSImage(contentsOfFile: args[1]),
      let cg = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
    print("{\"error\":\"cannot read the image\"}")
    exit(2)
}
let request = VNDetectBarcodesRequest()
request.symbologies = [.qr]
do {
    try VNImageRequestHandler(cgImage: cg, options: [:]).perform([request])
} catch {
    print("{\"error\":\"the reader failed\"}")
    exit(3)
}
let codes = (request.results ?? []).compactMap { $0.payloadStringValue }
let data = try! JSONSerialization.data(withJSONObject: ["codes": codes])
print(String(data: data, encoding: .utf8)!)
