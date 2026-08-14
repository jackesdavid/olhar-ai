// Join a tenant by scanning the QR from Settings → Users, or by typing the code (#181).
//
// This is the first screen a new person ever sees, and they arrive with nothing: no account, no
// idea what this app is, possibly standing in front of whoever handed them the phone. So it does
// as little as it can, in this order: point the camera, confirm WHICH organisation, then ask for
// a password. The organisation name is the one thing that makes a stranger's QR safe to act on.
import { useEffect, useRef, useState } from "react";
import { Platform, Pressable, StyleSheet, View } from "react-native";
import { CameraView, useCameraPermissions } from "expo-camera";
import { useRouter } from "expo-router";
import {
  acceptInvite,
  parseInvite,
  previewCode,
  previewInvite,
  type InvitePayload,
  type InvitePreview,
} from "@/invite";
import { storeSession } from "@/session";
import { registerThisDevice } from "@/push";
import { Body, Button, Card, Danger, Input, Muted, Screen, Title } from "@/styles/ui";
import { radius, space, useTheme } from "@/styles/theme";

type Phase =
  | { k: "scanning" }
  | { k: "code" }
  | { k: "confirm"; payload: InvitePayload; preview: InvitePreview }
  | { k: "joining" };

export default function Invite() {
  const [permission, requestPermission] = useCameraPermissions();
  const [phase, setPhase] = useState<Phase>({ k: "scanning" });
  const [code, setCode] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const router = useRouter();
  const t = useTheme();

  // A camera fires this many times a second, and every frame holding a QR is another call. Without
  // the latch the first scan starts a request and the next twenty start twenty more.
  const handling = useRef(false);

  useEffect(() => {
    if (permission && !permission.granted && permission.canAskAgain) void requestPermission();
  }, [permission, requestPermission]);

  async function onScanned(raw: string) {
    if (handling.current || phase.k !== "scanning") return;
    const payload = parseInvite(raw);
    // Not one of ours: stay silent and keep scanning. A camera sees Wi-Fi QRs and shop receipts,
    // and an error toast for each would make the screen unusable.
    if (!payload) return;
    handling.current = true;
    setError(null);
    try {
      const preview = await previewInvite(payload);
      setPhase({ k: "confirm", payload, preview });
      if (preview.email) setEmail(preview.email);
    } catch (e) {
      setError((e as Error).message);
      handling.current = false;
    }
  }

  async function submitCode() {
    if (handling.current || !code.trim()) return;
    handling.current = true;
    setError(null);
    try {
      const { payload, preview } = await previewCode(code);
      setPhase({ k: "confirm", payload, preview });
      if (preview.email) setEmail(preview.email);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      handling.current = false;
    }
  }

  async function join() {
    if (phase.k !== "confirm") return;
    const { payload, preview } = phase;
    if (password.length < 8) {
      setError("Use at least 8 characters.");
      return;
    }
    if (!preview.email && !email.includes("@")) {
      setError("Enter your email.");
      return;
    }
    setPhase({ k: "joining" });
    setError(null);
    try {
      const s = await acceptInvite(payload, password, preview.email ? undefined : email.trim());
      await storeSession(s.token);
      // Best effort, exactly like sign-in: an alert that never arrives is reported on the Alerts
      // tab rather than blocking someone from getting into the app.
      void registerThisDevice();
      router.replace("/(tabs)");
    } catch (e) {
      setError((e as Error).message);
      setPhase({ k: "confirm", payload, preview });
    }
  }

  const canScan = !!permission?.granted && phase.k === "scanning";

  return (
    <Screen>
      <View style={styles.body}>
        <Title>Join a workspace</Title>

        {phase.k === "scanning" && (
          <>
            <Muted>Scan the QR code from Settings → Users on the web app.</Muted>
            <View style={[styles.viewfinder, { borderColor: t.border, borderRadius: radius.xl }]}>
              {canScan ? (
                <CameraView
                  style={StyleSheet.absoluteFill}
                  barcodeScannerSettings={{ barcodeTypes: ["qr"] }}
                  onBarcodeScanned={(r) => void onScanned(r.data)}
                />
              ) : (
                <View style={styles.center}>
                  <Body>
                    {permission?.canAskAgain === false
                      ? "Camera access is off — allow it in Settings, or use the code instead."
                      : "Waiting for camera…"}
                  </Body>
                </View>
              )}
            </View>
            <Pressable onPress={() => setPhase({ k: "code" })} style={styles.link}>
              <Body>Type the code instead</Body>
            </Pressable>
          </>
        )}

        {phase.k === "code" && (
          <Card>
            <Muted>Enter the code shown next to the QR.</Muted>
            <Input
              placeholder="ABCD2345"
              autoCapitalize="characters"
              autoCorrect={false}
              value={code}
              onChangeText={setCode}
              onSubmitEditing={() => void submitCode()}
            />
            <Button label="Continue" onPress={() => void submitCode()} />
            <Pressable onPress={() => setPhase({ k: "scanning" })} style={styles.link}>
              <Muted>Scan instead</Muted>
            </Pressable>
          </Card>
        )}

        {(phase.k === "confirm" || phase.k === "joining") && (
          <Card>
            {/* The name is the whole point of asking before typing: it is what tells someone
                they are joining the organisation they think they are. */}
            <Title>{phase.k === "confirm" ? phase.preview.tenant_name : "Joining…"}</Title>
            <Muted>Choose a password and you are in.</Muted>
            {phase.k === "confirm" && !phase.preview.email && (
              <Input
                placeholder="Email"
                autoCapitalize="none"
                keyboardType="email-address"
                value={email}
                onChangeText={setEmail}
              />
            )}
            <Input
              placeholder="Password (at least 8 characters)"
              secureTextEntry
              value={password}
              onChangeText={setPassword}
              onSubmitEditing={() => void join()}
            />
            <Button label="Join" onPress={() => void join()} busy={phase.k === "joining"} />
          </Card>
        )}

        {error ? <Danger>{error}</Danger> : null}

        <Pressable onPress={() => router.replace("/login")} style={styles.link}>
          <Muted>I already have an account</Muted>
        </Pressable>
      </View>
    </Screen>
  );
}

const styles = StyleSheet.create({
  body: { padding: space.lg, gap: space.md },
  viewfinder: {
    aspectRatio: 1,
    overflow: "hidden",
    borderWidth: StyleSheet.hairlineWidth,
  },
  center: { flex: 1, alignItems: "center", justifyContent: "center", padding: space.lg },
  link: { alignSelf: "center", paddingVertical: space.md },
});

export const INVITE_PLATFORM = Platform.OS;
