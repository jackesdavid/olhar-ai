// Embedded edge (card #144) — the phone runs the REAL sentinel-edge, enrolled as a first-class box.
// Boot self-registers this phone (#143) and starts the edge with that identity, so it takes the full
// path: pulls its tenant's cameras from control, records them, and reports health. Recording drives the
// real /recording/start + /recording/stop flow — the clip uploads to R2 on its own, no /dev shortcuts.
import { useState } from "react";
import { ScrollView, StyleSheet } from "react-native";

import { hasEmbeddedEdge, startEdge } from "sentinel-edge";
import { ensureRegistered } from "@/lightEdge";
import { CONTROL_BASE } from "@/config";
import { Button, Card, Danger, Heading, Input, Muted, Strong } from "@/styles/ui";
import { space } from "@/styles/theme";

const EDGE = "http://127.0.0.1:8788";

export default function EdgeTest() {
  const [status, setStatus] = useState("idle");
  const [health, setHealth] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The camera IP — the enrolled edge already has its RTSP URL + credentials from control.
  const [cameraIp, setCameraIp] = useState("192.168.0.7");
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
      // Enroll: self-register this phone as a box (#143), then boot the edge WITH that identity so it
      // runs the full path — pulls its tenant's cameras from control, records, reports health.
      const edge = await ensureRegistered("iPhone edge");
      await startEdge(edge.boxToken, CONTROL_BASE);
      setStatus("polling");
      const ok = await pollHealth();
      setHealth(ok);
      setStatus("up");
    } catch (e) {
      setError((e as Error).message);
      setStatus("idle");
    }
  }

  async function recordCamera() {
    setRec(null);
    setRecBusy(true);
    const ip = cameraIp.trim();
    try {
      // The real recording flow: start a ring on the pulled camera, hold ~8s, stop. Both endpoints
      // return a plain data object ({clip_id,…}) and signal success with the HTTP status — there is
      // no {ok} envelope — so `post` throws on a non-2xx and we trust a clean return. On stop the edge
      // finalizes the clip and uploads it to R2 itself (Bearer BOX_TOKEN); nothing here touches that.
      setRec("starting recording…");
      const start = await post("/recording/start", { camera_ip: ip });
      await new Promise((res) => setTimeout(res, 8000));
      setRec("stopping…");
      const stop = await post("/recording/stop", { camera_ip: ip, clip_id: start.clip_id });
      setRec(`Recorded ${ip} → clip ${stop.clip_id ?? start.clip_id ?? "?"} (uploading to R2 in the background)`);
    } catch (e) {
      setRec(`could not record: ${(e as Error).message}`);
    } finally {
      setRecBusy(false);
    }
  }

  return (
    <ScrollView
      contentContainerStyle={styles.body}
      // The camera field sits mid-screen; without these the keyboard covers it while typing.
      automaticallyAdjustKeyboardInsets
      keyboardShouldPersistTaps="handled"
      keyboardDismissMode="interactive"
    >
      <Heading>Embedded edge</Heading>

      {/* Record card first — its input must stay above the keyboard. */}
      <Card>
        <Strong>Record a camera</Strong>
        <Muted>Records ~8s from one of this box's cameras and uploads the clip to the cloud. Free the camera first (stop the Mac edge).</Muted>
        <Input value={cameraIp} onChangeText={setCameraIp} autoCapitalize="none" autoCorrect={false} placeholder="camera IP (e.g. 192.168.0.7)" />
        {rec ? <Muted>{rec}</Muted> : null}
      </Card>
      <Button label="Record (8s)" onPress={recordCamera} busy={recBusy} disabled={status !== "up"} />

      <Card>
        <Strong>{label(status)}</Strong>
        {health ? <Muted>{health}</Muted> : null}
        {error ? <Danger>{error}</Danger> : null}
      </Card>
      <Button label="Start embedded edge" onPress={boot} busy={status === "starting" || status === "polling"} />
    </ScrollView>
  );
}

/** POST a JSON body to the local edge. The edge signals success with the HTTP status and returns a
 *  plain data object (no {ok} envelope); on a non-2xx the body is a bare error string, so surface it. */
async function post(path: string, body: unknown): Promise<any> {
  const r = await fetch(`${EDGE}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(90000),
  });
  const text = await r.text();
  if (!r.ok) throw new Error(text || `HTTP ${r.status}`);
  try {
    return JSON.parse(text);
  } catch {
    return {};
  }
}

/** Poll the edge's /health for up to ~10s — it binds shortly after boot. */
async function pollHealth(): Promise<string> {
  for (let i = 0; i < 20; i++) {
    try {
      const r = await fetch(`${EDGE}/health`, { signal: AbortSignal.timeout(1000) });
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
