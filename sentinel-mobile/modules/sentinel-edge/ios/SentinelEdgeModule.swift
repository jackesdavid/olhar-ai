// Bridges the phone app to the embedded `sentinel-edge` Rust library (card #144). The edge is linked
// as a static lib; its C ABI is called by name — no header/modulemap. `startEdge(boxToken, control)`
// boots the edge ENROLLED (box token from self-register, #143), so it runs the full path — pull
// cameras, record, report health — and talks to the given control URL.
import ExpoModulesCore

@_silgen_name("start_edge")
func rust_start_edge(_ boxToken: UnsafePointer<CChar>?, _ controlUrl: UnsafePointer<CChar>?)
@_silgen_name("stop_edge") func rust_stop_edge()

public class SentinelEdgeModule: Module {
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
  }
}
