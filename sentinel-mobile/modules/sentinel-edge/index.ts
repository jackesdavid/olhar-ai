// JS binding to the native SentinelEdge module (card #144). Present only on a device build that
// linked the Rust edge lib; guarded so the managed/Expo-Go path degrades instead of crashing.
import { requireOptionalNativeModule } from "expo-modules-core";

const Native = requireOptionalNativeModule("SentinelEdge");

/** Is the embedded edge available (a device build that linked libsentinel_edge.a)? */
export const hasEmbeddedEdge = Native != null;

/** Boot the embedded edge ENROLLED with this phone's box token + control URL (#143/#144), so it runs
 *  the full edge — pulls its cameras, records, reports health — and binds localhost:8788. */
export async function startEdge(boxToken: string, controlUrl: string): Promise<void> {
  if (!Native) throw new Error("embedded edge not available in this build");
  await Native.startEdge(boxToken, controlUrl);
}

/** Request the embedded edge to stop. */
export async function stopEdge(): Promise<void> {
  if (!Native) return;
  await Native.stopEdge();
}
