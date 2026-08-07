// Bridges the phone app to the embedded `sentinel-edge` Rust library (card #144). The edge is linked
// as a static lib; its C ABI is called by name — no header/modulemap. `startEdge(boxToken, control)`
// boots the edge ENROLLED (box token from self-register, #143), so it runs the full path — pull
// cameras, record, report health — and talks to the given control URL.
import ExpoModulesCore

@_silgen_name("start_edge")
func rust_start_edge(_ boxToken: UnsafePointer<CChar>?, _ controlUrl: UnsafePointer<CChar>?)
@_silgen_name("stop_edge") func rust_stop_edge()

public class SentinelEdgeModule: Module {
  /// The phone's own camera, feeding the edge's frame slot (#175 Phase 1). Held here so the session
  /// outlives the JS call that started it — a local would be deallocated and the capture would stop
  /// a moment after it began, which looks like a camera that never worked.
  private let deviceCamera = DeviceCamera()

  public func definition() -> ModuleDefinition {
    Name("SentinelEdge")

    AsyncFunction("startEdge") { (boxToken: String, controlUrl: String) in
      boxToken.withCString { t in
        controlUrl.withCString { c in
          rust_start_edge(t, c)
        }
      }
    }

    AsyncFunction("stopEdge") {
      rust_stop_edge()
    }

    // Feed the phone's own camera into the running edge as an ordinary camera. `cameraId` must
    // match a device in the tenant's list whose `rtsp_url` is `device:…`; that is what makes the
    // frame hub start for it and what stops anything trying to dial it.
    AsyncFunction("startDeviceCamera") { (cameraId: String, facing: String, fps: Double, quality: Double) in
      try self.deviceCamera.start(cameraId: cameraId, facing: facing, fps: fps, quality: quality)
    }

    AsyncFunction("stopDeviceCamera") {
      self.deviceCamera.stop()
    }

    Function("isDeviceCameraRunning") { () -> Bool in
      self.deviceCamera.running
    }
  }
}
