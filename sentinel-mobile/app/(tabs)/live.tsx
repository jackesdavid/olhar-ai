// Live — Phase 2. Camera list from control, playback through the gateway WebSocket.
//
// Playback runs in a WebView because the transport is fragmented MP4 over a WebSocket and React
// Native has no MediaSource; see src/live/msePlayer.ts for why. Requires `live.view`, which is
// Admin-only by default, so a 403 gets an honest message rather than a spinner forever.
import { useEffect, useMemo, useState } from "react";
import { ActivityIndicator, Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { WebView } from "react-native-webview";
import { GATEWAY_BASE } from "@/config";
import { listDevices, streamName, type Device } from "@/data";
import { currentToken } from "@/session";
import { playerHtml, type PlayerEvent } from "@/live/msePlayer";
import { Danger, Heading, Muted, Screen, Strong } from "@/styles/ui";
import { radius, space, useTheme } from "@/styles/theme";

type Phase = "connecting" | "live" | "stalled" | "error";

export default function Live() {
  const [devices, setDevices] = useState<Device[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [hd, setHd] = useState(false);
  const [phase, setPhase] = useState<Phase>("connecting");
  const [playerError, setPlayerError] = useState<string | null>(null);
  /** Bumped to remount the WebView, which is how a stream restarts: a fresh socket and a fresh
   *  MediaSource. Reusing a MediaSource that has already errored does not work. */
  const [attempt, setAttempt] = useState(0);
  const t = useTheme();

  useEffect(() => {
    listDevices()
      .then((d) => {
        setDevices(d);
        if (d.length > 0) setSelected(d[0].ip);
      })
      .catch((e) => setLoadError((e as Error).message));
  }, []);

  const token = currentToken();
  const wsBase = GATEWAY_BASE.replace(/^http/, "ws");

  // Remounting the WebView is how we restart a stream: a new key throws away the old socket and
  // MediaSource instead of trying to reuse them.
  const playerKey = `${selected}-${hd ? "hd" : "sd"}-${attempt}`;
  const html = useMemo(
    () => (selected && token ? playerHtml({ wsBase, stream: streamName(selected, hd), token }) : null),
    [selected, hd, token, wsBase],
  );

  function onMessage(raw: string) {
    let msg: PlayerEvent;
    try {
      msg = JSON.parse(raw);
    } catch {
      return;
    }
    if (msg.type === "phase") setPhase(msg.phase);
    if (msg.type === "error") {
      setPhase("error");
      setPlayerError(msg.message);
    }
  }

  // A live stream will drop — a decode error, a socket close, the box restarting. What matters is
  // that it comes back on its own: an operator checking a camera should never have to know to
  // leave the tab and come back. Restart on any non-playing state, backing off a little so a
  // camera that is genuinely down does not spin.
  useEffect(() => {
    if (phase !== "error" && phase !== "stalled") return;
    const delay = Math.min(1500 * (attempt + 1), 8000);
    const timer = setTimeout(() => {
      setPlayerError(null);
      setPhase("connecting");
      setAttempt((n) => n + 1);
    }, delay);
    return () => clearTimeout(timer);
  }, [phase]);

  function badgeLabel(): string {
    if (phase === "live") return "LIVE";
    if (phase === "error" || phase === "stalled") return "Reconnecting…";
    return attempt > 0 ? "Reconnecting…" : "Connecting…";
  }

  if (loadError) {
    return (
      <Screen style={{ padding: space.xl, justifyContent: "center", gap: space.md }}>
        <Heading>Live</Heading>
        <Danger>{loadError}</Danger>
        <Muted>Live video needs the live.view permission, which is Admin-only by default.</Muted>
      </Screen>
    );
  }

  if (!devices) {
    return (
      <Screen style={{ alignItems: "center", justifyContent: "center" }}>
        <ActivityIndicator color={t.mutedForeground} />
      </Screen>
    );
  }

  if (devices.length === 0) {
    return (
      <Screen style={{ padding: space.xl, justifyContent: "center", gap: space.md }}>
        <Heading>Live</Heading>
        <Muted>No cameras yet. Add one from the web app and it appears here.</Muted>
      </Screen>
    );
  }

  return (
    <Screen>
      <View style={{ aspectRatio: 16 / 9, backgroundColor: "#000" }}>
        {html ? (
          <WebView
            key={playerKey}
            // Inline HTML loads as an opaque origin, and WKWebView blocks a WebSocket from one.
            // Giving the page the gateway's own origin makes the connection same-origin.
            source={{ html, baseUrl: GATEWAY_BASE }}
            originWhitelist={["*"]}
            style={{ flex: 1, backgroundColor: "#000" }}
            allowsInlineMediaPlayback
            mediaPlaybackRequiresUserAction={false}
            onMessage={(e) => onMessage(e.nativeEvent.data)}
          />
        ) : null}
        <View
          style={{
            position: "absolute",
            top: space.sm,
            left: space.sm,
            backgroundColor: "#000000AA",
            borderRadius: radius.pill,
            paddingHorizontal: space.md,
            paddingVertical: space.xs,
          }}
        >
          <Text style={{ color: "#FFF", fontSize: 12, fontWeight: "600" }}>{badgeLabel()}</Text>
        </View>
        <Pressable
          onPress={() => setHd((v) => !v)}
          style={{
            position: "absolute",
            top: space.sm,
            right: space.sm,
            backgroundColor: "#000000AA",
            borderRadius: radius.pill,
            paddingHorizontal: space.md,
            paddingVertical: space.xs,
          }}
        >
          <Text style={{ color: "#FFF", fontSize: 12, fontWeight: "600" }}>{hd ? "HD" : "SD"}</Text>
        </Pressable>
      </View>

      {playerError ? (
        <View style={{ padding: space.lg }}>
          <Danger>{playerError}</Danger>
        </View>
      ) : null}

      <ScrollView contentContainerStyle={{ padding: space.lg, gap: space.sm }}>
        <Heading>Cameras</Heading>
        {devices.map((d) => {
          const on = d.ip === selected;
          return (
            <Pressable
              key={d.ip}
              onPress={() => {
                setSelected(d.ip);
                setPhase("connecting");
                setPlayerError(null);
                setAttempt(0);
              }}
              style={{
                backgroundColor: on ? t.elev7 : t.card,
                borderColor: t.border,
                borderWidth: StyleSheet.hairlineWidth,
                borderRadius: radius.lg,
                padding: space.md,
              }}
            >
              <Strong>{d.name || d.ip}</Strong>
              <Muted>{d.ip}</Muted>
            </Pressable>
          );
        })}
      </ScrollView>
    </Screen>
  );
}
