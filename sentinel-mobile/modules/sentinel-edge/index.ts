// JS binding to the native SentinelEdge module (card #144). Present only on a device build that
// linked the Rust edge lib; guarded so the managed/Expo-Go path degrades instead of crashing.
import { requireOptionalNativeModule } from "expo-modules-core";

const Native = requireOptionalNativeModule("SentinelEdge");

/** Is the embedded edge available (a device build that linked libsentinel_edge.a)? */
export const hasEmbeddedEdge = Native != null;

/** Boot the embedded edge — it binds its HTTP server on localhost:8788, like an appliance. */
export async function startEdge(): Promise<void> {
  if (!Native) throw new Error("embedded edge not available in this build");
  await Native.startEdge();
}

/** Request the embedded edge to stop. */
export async function stopEdge(): Promise<void> {
  if (!Native) return;
  await Native.stopEdge();
}
