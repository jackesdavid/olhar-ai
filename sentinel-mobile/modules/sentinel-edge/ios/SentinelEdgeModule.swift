// Bridges the phone app to the embedded `sentinel-edge` Rust library (card #144). The edge is linked
// as a static lib (libsentinel_edge.a); its C ABI entry points are called by name — no header or
// modulemap needed. `startEdge` boots the edge (it binds its HTTP server on localhost:8788, exactly
// as an appliance does); the app then talks to it over localhost like the web app talks to a box.
import ExpoModulesCore

// The Rust C ABI, resolved from the linked static library at link time.
@_silgen_name("start_edge") func rust_start_edge()
@_silgen_name("stop_edge") func rust_stop_edge()

public class SentinelEdgeModule: Module {
  public func definition() -> ModuleDefinition {
    Name("SentinelEdge")

    AsyncFunction("startEdge") {
      rust_start_edge()
    }

    AsyncFunction("stopEdge") {
      rust_stop_edge()
    }
  }
}
