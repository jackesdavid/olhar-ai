// Live — camera list from control, playback through the gateway WebSocket.
//
// Playback runs in a WebView because the transport is fragmented MP4 over a WebSocket and React
// Native has no MediaSource; see src/live/msePlayer.ts. Requires `live.view`, which is Admin-only
// by default, so a 403 gets an honest message rather than a spinner forever.
//
// The player itself lives in `src/live/LivePlayer.tsx` — shared with the fullscreen view, so the
// reconnect policy exists once.
import { useEffect, useState } from "react";
import { ActivityIndicator, Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { useRouter } from "expo-router";
import { listDevices, type Device } from "@/data";
import { LivePlayer, phaseLabel, type Phase } from "@/live/LivePlayer";
import { Danger, Heading, Muted, Screen, Strong } from "@/styles/ui";
import { radius, space, useTheme } from "@/styles/theme";

export default function Live() {
  const [devices, setDevices] = useState<Device[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [hd, setHd] = useState(false);
  const [phase, setPhase] = useState<Phase>("connecting");
  const [retried, setRetried] = useState(false);
  const [playerError, setPlayerError] = useState<string | null>(null);
  const router = useRouter();
  const t = useTheme();

  useEffect(() => {
    listDevices()
      .then((d) => {
        setDevices(d);
        if (d.length > 0) setSelected(d[0].ip);
      })
      .catch((e) => setLoadError((e as Error).message));
  }, []);

  const current = devices?.find((d) => d.ip === selected);

  function openFullscreen() {
    if (!selected) return;
    router.push({
      pathname: "/live-full",
      params: { ip: selected, name: current?.name || selected, hd: hd ? "1" : "0" },
    });
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
        {selected ? (
          <LivePlayer
            ip={selected}
            hd={hd}
            onPhase={(p) => {
              setPhase(p);
              if (p === "connecting") setRetried(true);
            }}
            onError={setPlayerError}
          />
        ) : null}

        {/* The picture itself opens fullscreen. A dedicated expand button would be one more thing
            to find on a 16:9 strip; tapping the video is what people already try. */}
        <Pressable style={StyleSheet.absoluteFill} onPress={openFullscreen} />

        <View style={[styles.pill, { top: space.sm, left: space.sm }]}>
          <Text style={styles.pillText}>{phaseLabel(phase, retried)}</Text>
        </View>
        <Pressable
          onPress={() => setHd((v) => !v)}
          style={[styles.pill, { top: space.sm, right: space.sm }]}
        >
          <Text style={styles.pillText}>{hd ? "HD" : "SD"}</Text>
        </Pressable>
        <Pressable
          onPress={openFullscreen}
          style={[styles.pill, { bottom: space.sm, right: space.sm }]}
        >
          <Text style={styles.pillText}>⤢</Text>
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
                setPlayerError(null);
                setRetried(false);
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

const styles = StyleSheet.create({
  pill: {
    position: "absolute",
    backgroundColor: "#000000AA",
    borderRadius: 999,
    paddingHorizontal: space.md,
    paddingVertical: space.xs,
  },
  pillText: { color: "#FFF", fontSize: 12, fontWeight: "600" },
});
