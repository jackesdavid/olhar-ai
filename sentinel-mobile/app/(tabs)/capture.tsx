// Capture — the phone as an on-demand face-capture edge (card #142). Front camera, foreground only:
// while "Capturing" is on, it snaps a photo on an interval and pushes it to the cloud, which detects
// and crops the face (the existing embed → recognize → StoreFace pipeline). Closing / stopping ends
// it — the iOS-friendly boundary. On-device face auto-detection (vision-camera + MLKit) is a later
// enhancement; v1 sends the full photo and lets the proven cloud detector do the crop.
import { useCallback, useEffect, useRef, useState } from "react";
import { ActivityIndicator, Pressable, StyleSheet, Text, View } from "react-native";
import { CameraView, useCameraPermissions } from "expo-camera";
import * as ImageManipulator from "expo-image-manipulator";

import { ensureRegistered, pushFace, type LightEdge } from "@/lightEdge";
import { Danger, Heading, Muted, Screen, Strong } from "@/styles/ui";
import { radius, space, useTheme } from "@/styles/theme";

/** How often a photo is snapped while capturing. Slow enough not to flood the pipeline or the
 *  network, fast enough to catch a face that steps in front of the phone. */
const SNAP_INTERVAL_MS = 1800;
/** Downscale before upload: a full-res iPhone photo is multi-MB of base64 over cellular; the cloud
 *  face detector needs nothing near that resolution. */
const MAX_WIDTH = 720;

type Status = "idle" | "registering" | "ready" | "capturing";

export default function Capture() {
  const t = useTheme();
  const [permission, requestPermission] = useCameraPermissions();
  const [status, setStatus] = useState<Status>("idle");
  const [edge, setEdge] = useState<LightEdge | null>(null);
  const [sent, setSent] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const camera = useRef<CameraView>(null);
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);
  // The latest edge, read from inside the interval closure without re-arming it.
  const edgeRef = useRef<LightEdge | null>(null);
  edgeRef.current = edge;

  // Register this phone as a light-edge box once (idempotent client-side).
  useEffect(() => {
    let alive = true;
    setStatus("registering");
    ensureRegistered("iPhone — front camera")
      .then((e) => {
        if (!alive) return;
        setEdge(e);
        setStatus("ready");
      })
      .catch((e) => {
        if (!alive) return;
        setError((e as Error).message);
        setStatus("idle");
      });
    return () => {
      alive = false;
    };
  }, []);

  const stop = useCallback(() => {
    if (timer.current) {
      clearInterval(timer.current);
      timer.current = null;
    }
    setStatus((s) => (s === "capturing" ? "ready" : s));
  }, []);

  // Stop capturing when the screen unmounts — capture must never outlive the foreground view.
  useEffect(() => stop, [stop]);

  async function snapAndPush() {
    const e = edgeRef.current;
    if (!e || !camera.current) return;
    try {
      const shot = await camera.current.takePictureAsync({ base64: false, quality: 0.6, skipProcessing: true });
      if (!shot?.uri) return;
      // Resize + re-encode to a modest JPEG; `base64` here is the resized image, not the full frame.
      const small = await ImageManipulator.manipulateAsync(shot.uri, [{ resize: { width: MAX_WIDTH } }], {
        base64: true,
        compress: 0.6,
        format: ImageManipulator.SaveFormat.JPEG,
      });
      if (!small.base64) return;
      // bbox / kps / confidence left empty: the cloud pipeline detects and crops the face itself
      // (embed_face_crop's detect+fallback), so the phone need not run a detector for v1.
      const ok = await pushFace(e, {
        bbox: [0, 0, 0, 0],
        confidence: 0,
        faceRatio: 0,
        kps: [],
        frameB64: small.base64,
      });
      if (ok) setSent((n) => n + 1);
    } catch (err) {
      // One failed frame is not worth ending the session — surface it but keep going.
      setError((err as Error).message);
    }
  }

  function start() {
    if (status !== "ready") return;
    setError(null);
    setSent(0);
    setStatus("capturing");
    void snapAndPush();
    timer.current = setInterval(() => void snapAndPush(), SNAP_INTERVAL_MS);
  }

  if (!permission) return renderCentered(<ActivityIndicator color={t.foreground} />);
  if (!permission.granted) {
    return renderCentered(
      <View style={styles.center}>
        <Muted>Camera access is needed to capture faces.</Muted>
        <Pressable
          onPress={requestPermission}
          style={[styles.button, { backgroundColor: t.foreground, borderRadius: radius.md }]}
        >
          <Text style={{ color: t.background, fontWeight: "600" }}>Grant camera access</Text>
        </Pressable>
      </View>,
    );
  }

  const capturing = status === "capturing";
  return (
    <Screen>
      <Heading>Face capture</Heading>
      <Muted>Point the front camera at a face. The phone acts as a temporary edge while this screen is open.</Muted>

      <View style={[styles.preview, { borderColor: t.border, borderRadius: radius.lg }]}>
        <CameraView ref={camera} style={StyleSheet.absoluteFill} facing="front" />
        {capturing ? <View style={[styles.badge, { backgroundColor: t.foreground }]} /> : null}
      </View>

      <Strong>{captureLabel(status, sent)}</Strong>
      {error ? <Danger>{error}</Danger> : null}

      <Pressable
        onPress={capturing ? stop : start}
        disabled={status === "registering"}
        style={[
          styles.button,
          { backgroundColor: capturing ? t.destructive : t.foreground, borderRadius: radius.md, opacity: status === "registering" ? 0.5 : 1 },
        ]}
      >
        <Text style={{ color: t.background, fontWeight: "600" }}>{capturing ? "Stop capturing" : "Start capturing"}</Text>
      </Pressable>
    </Screen>
  );
}

/** The status line — a named helper so the markup carries no branching logic (house style). */
function captureLabel(status: Status, sent: number): string {
  if (status === "registering") return "Registering this phone…";
  if (status === "capturing") return `Capturing — ${sent} face${sent === 1 ? "" : "s"} sent`;
  if (sent > 0) return `Stopped — ${sent} sent this session`;
  return "Ready";
}

function renderCentered(node: React.ReactNode) {
  return <Screen><View style={styles.center}>{node}</View></Screen>;
}

const styles = StyleSheet.create({
  center: { flex: 1, alignItems: "center", justifyContent: "center", gap: space.md, padding: space.lg },
  preview: { aspectRatio: 3 / 4, overflow: "hidden", borderWidth: StyleSheet.hairlineWidth, marginVertical: space.md },
  badge: { position: "absolute", top: space.sm, right: space.sm, width: 12, height: 12, borderRadius: 6 },
  button: { paddingVertical: space.md, alignItems: "center", marginTop: space.sm },
});
