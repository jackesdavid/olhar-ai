// Embedded edge test (card #144) — the phone runs the real sentinel-edge. This screen boots it via
// the native module and confirms its HTTP server answers on localhost, which is the first on-device
// proof that the Rust edge runtime runs on iOS. GStreamer pipelines come after this passes.
import { useState } from "react";
import { ScrollView, StyleSheet } from "react-native";

import { hasEmbeddedEdge, startEdge } from "sentinel-edge";
import { Button, Card, Danger, Heading, Input, Muted, Strong } from "@/styles/ui";
import { space } from "@/styles/theme";

const HEALTH_URL = "http://127.0.0.1:8788/health";
const GST_URL = "http://127.0.0.1:8788/gst-selftest";
const RECORD_URL = "http://127.0.0.1:8788/dev/record";

export default function EdgeTest() {
  const [status, setStatus] = useState("idle");
  const [health, setHealth] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [gst, setGst] = useState<string | null>(null);
  const [gstBusy, setGstBusy] = useState(false);
  // RTSP URL kept in state (not committed) so camera credentials never live in source.
  const [rtsp, setRtsp] = useState("rtsp://user:pass@192.168.0.7:554/stream2");
  const [rec, setRec] = useState<string | null>(null);
  const [recBusy, setRecBusy] = useState(false);

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

  async function testGst() {
    setGst(null);
    setGstBusy(true);
    try {
      const r = await fetch(GST_URL, { signal: AbortSignal.timeout(15000) });
      const j = await r.json();
      setGst(j.ok ? "GStreamer OK — all 13 elements built ✓" : `GStreamer FAILED — ${j.error}`);
    } catch (e) {
      setGst(`could not reach the edge: ${(e as Error).message}`);
    } finally {
      setGstBusy(false);
    }
  }

  async function recordCamera() {
    setRec(null);
    setRecBusy(true);
    try {
      const u = `${RECORD_URL}?url=${encodeURIComponent(rtsp)}&secs=8`;
      const r = await fetch(u, { method: "POST", signal: AbortSignal.timeout(25000) });
      const j = await r.json();
      setRec(j.ok ? `Recorded ${fmtBytes(j.bytes)} in ${j.secs}s → ${j.path}` : `Failed — ${j.error}`);
    } catch (e) {
      setRec(`could not reach the edge: ${(e as Error).message}`);
    } finally {
      setRecBusy(false);
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

      <Card>
        <Strong>GStreamer</Strong>
        <Muted>Instantiates every pipeline element — proves the iOS static plugin registration.</Muted>
        {gst ? <Muted>{gst}</Muted> : null}
      </Card>
      <Button label="Test GStreamer" onPress={testGst} busy={gstBusy} disabled={status !== "up"} />

      <Card>
        <Strong>Record a camera</Strong>
        <Muted>Dials the RTSP camera directly and records 8s on the phone. Free the camera first (stop the Mac edge).</Muted>
        <Input value={rtsp} onChangeText={setRtsp} autoCapitalize="none" autoCorrect={false} placeholder="rtsp://…" />
        {rec ? <Muted>{rec}</Muted> : null}
      </Card>
      <Button label="Record camera (8s)" onPress={recordCamera} busy={recBusy} disabled={status !== "up"} />
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

function fmtBytes(n: number): string {
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
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
