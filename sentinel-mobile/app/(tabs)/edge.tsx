// Embedded edge test (card #144) — the phone runs the real sentinel-edge. This screen boots it via
// the native module and confirms its HTTP server answers on localhost, which is the first on-device
// proof that the Rust edge runtime runs on iOS. GStreamer pipelines come after this passes.
import { useState } from "react";
import { ScrollView, StyleSheet } from "react-native";

import { hasEmbeddedEdge, startEdge } from "sentinel-edge";
import { Button, Card, Danger, Heading, Muted, Strong } from "@/styles/ui";
import { space } from "@/styles/theme";

const HEALTH_URL = "http://127.0.0.1:8788/health";

export default function EdgeTest() {
  const [status, setStatus] = useState("idle");
  const [health, setHealth] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function boot() {
    setError(null);
    setHealth(null);
    if (!hasEmbeddedEdge) {
      setError("This build did not link the embedded edge. Run build-ios-lib.sh + a device build.");
      return;
    }
    try {
      setStatus("starting");
      await startEdge();
      setStatus("polling");
      // The edge binds its HTTP server on boot; poll /health until it answers.
      const ok = await pollHealth();
      setHealth(ok);
      setStatus("up");
    } catch (e) {
      setError((e as Error).message);
      setStatus("idle");
    }
  }

  return (
    <ScrollView contentContainerStyle={styles.body}>
      <Heading>Embedded edge</Heading>
      <Muted>Boots the real sentinel-edge inside the app and checks its HTTP server (localhost:8788).</Muted>

      <Card>
        <Strong>{label(status)}</Strong>
        {health ? <Muted>{health}</Muted> : null}
        {error ? <Danger>{error}</Danger> : null}
      </Card>

      <Button label="Start embedded edge" onPress={boot} busy={status === "starting" || status === "polling"} />
    </ScrollView>
  );
}

/** Poll the edge's /health for up to ~10s — it binds shortly after boot. */
async function pollHealth(): Promise<string> {
  for (let i = 0; i < 20; i++) {
    try {
      const r = await fetch(HEALTH_URL, { signal: AbortSignal.timeout(1000) });
      if (r.ok) return await r.text();
    } catch {
      // not up yet
    }
    await new Promise((res) => setTimeout(res, 500));
  }
  throw new Error("edge HTTP server did not answer within 10s");
}

function label(status: string): string {
  if (status === "starting") return "Starting the edge…";
  if (status === "polling") return "Waiting for the HTTP server…";
  if (status === "up") return "Edge is up — /health answered ✓";
  return "Idle";
}

const styles = StyleSheet.create({
  body: { padding: space.lg, gap: space.lg },
});
