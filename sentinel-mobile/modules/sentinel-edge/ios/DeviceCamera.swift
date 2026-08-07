// The phone's own camera as an edge capture source (#175 Phase 1).
//
// The edge reads every camera from one place: a per-camera slot holding the freshest JPEG, which a
// GStreamer RTSP pipeline normally fills. This class fills that same slot from `AVFoundation`, so
// the phone's front/back camera becomes an ordinary camera to everything downstream — the hub
// decodes it once, MOG2 decides motion, and a qualifying frame runs YuNet and emits `face_detected`
// with the #126 quality signals. No parallel pipeline, no second code path to keep in step.
//
// Why a JPEG and not the raw pixel buffer: the slot's contract is "the freshest encoded frame", and
// every other producer honours it. Handing over BGRA would mean teaching the hub a second input
// shape for one platform. The encode is hardware-backed and happens at the throttled rate, not the
// sensor's.
import AVFoundation
import CoreImage
import Foundation

@_silgen_name("push_camera_frame")
func rust_push_camera_frame(_ cameraId: UnsafePointer<CChar>?, _ jpeg: UnsafePointer<UInt8>?, _ len: Int) -> Bool

/// Owns the capture session and pushes frames into the edge. One instance per app; starting twice
/// is a no-op rather than a second session, because two sessions on one device is how you get a
/// black preview and a thermal warning.
final class DeviceCamera: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
  private let session = AVCaptureSession()
  private let queue = DispatchQueue(label: "olharai.device-camera", qos: .userInitiated)
  private let ciContext = CIContext(options: [.useSoftwareRenderer: false])

  private var cameraId = ""
  private var quality: CGFloat = 0.6
  /// Minimum gap between pushes. The sensor runs at 30 fps; the edge's motion detector reads at its
  /// own (much lower) rate, so encoding every frame would burn battery producing buffers nobody
  /// reads. This is the throttle that makes the phone viable as a continuous source.
  private var minInterval: TimeInterval = 0.2
  private var lastPush = Date.distantPast
  private(set) var running = false

  /// Start capturing into the edge slot named `cameraId`. `facing` is "front" or "back".
  /// Throws when the device has no such camera or the session cannot be configured — the caller
  /// surfaces it, because silently capturing nothing looks identical to a camera pointed at a wall.
  func start(cameraId: String, facing: String, fps: Double, quality: Double) throws {
    if running { return }
    self.cameraId = cameraId
    self.quality = CGFloat(max(0.1, min(1.0, quality)))
    self.minInterval = fps > 0 ? 1.0 / fps : 0.2

    let position: AVCaptureDevice.Position = (facing == "back") ? .back : .front
    guard let device = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: position) else {
      throw NSError(domain: "DeviceCamera", code: 1,
                    userInfo: [NSLocalizedDescriptionKey: "no \(facing) camera on this device"])
    }

    session.beginConfiguration()
    session.sessionPreset = .hd1280x720

    for input in session.inputs { session.removeInput(input) }
    for output in session.outputs { session.removeOutput(output) }

    let input = try AVCaptureDeviceInput(device: device)
    guard session.canAddInput(input) else {
      session.commitConfiguration()
      throw NSError(domain: "DeviceCamera", code: 2,
                    userInfo: [NSLocalizedDescriptionKey: "cannot add \(facing) camera input"])
    }
    session.addInput(input)

    let output = AVCaptureVideoDataOutput()
    output.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA]
    // Drop rather than queue. A late frame is worthless to a motion detector, and a growing queue
    // is how a capture session turns into a memory leak under thermal throttling.
    output.alwaysDiscardsLateVideoFrames = true
    output.setSampleBufferDelegate(self, queue: queue)
    guard session.canAddOutput(output) else {
      session.commitConfiguration()
      throw NSError(domain: "DeviceCamera", code: 3,
                    userInfo: [NSLocalizedDescriptionKey: "cannot add video output"])
    }
    session.addOutput(output)

    if let connection = output.connection(with: .video) {
      // Portrait, and un-mirrored on the front camera. Mirroring matters beyond looks: the pose
      // estimate reads landmark geometry, so a flipped frame reports the yaw of a person who is not
      // there.
      if connection.isVideoRotationAngleSupported(90) { connection.videoRotationAngle = 90 }
      if connection.isVideoMirroringSupported { connection.isVideoMirrored = false }
    }

    session.commitConfiguration()
    queue.async { [weak self] in self?.session.startRunning() }
    running = true
  }

  func stop() {
    guard running else { return }
    running = false
    queue.async { [weak self] in self?.session.stopRunning() }
  }

  func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer,
                     from connection: AVCaptureConnection) {
    let now = Date()
    guard now.timeIntervalSince(lastPush) >= minInterval else { return }
    guard let pixels = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
    let image = CIImage(cvPixelBuffer: pixels)
    guard let space = image.colorSpace ?? CGColorSpace(name: CGColorSpace.sRGB) else { return }
    guard let jpeg = ciContext.jpegRepresentation(of: image, colorSpace: space,
                                                  options: [kCGImageDestinationLossyCompressionQuality as CIImageRepresentationOption: quality])
    else { return }
    lastPush = now

    // The Rust side copies before returning, so this buffer can go out of scope immediately.
    cameraId.withCString { id in
      _ = jpeg.withUnsafeBytes { raw -> Bool in
        guard let base = raw.bindMemory(to: UInt8.self).baseAddress else { return false }
        return rust_push_camera_frame(id, base, raw.count)
      }
    }
  }
}
