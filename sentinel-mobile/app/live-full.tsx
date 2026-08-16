// Fullscreen live video.
//
// A camera is 16:9 and a phone held upright is not, so "fullscreen" on a portrait screen means a
// letterboxed strip with black above and below — bigger than the tab, but not what anyone means by
// the word. This screen rotates to landscape and gives the video the whole device.
//
// It locks the orientation itself and restores portrait on the way out, rather than letting the app
// rotate freely: every other screen is laid out for portrait, and a Device Gate arriving sideways
// while someone is watching a camera would be worse than no rotation at all.
import { useEffect, useState } from "react";
import { Pressable, StatusBar, StyleSheet, Text, View } from "react-native";
import * as ScreenOrientation from "expo-screen-orientation";
import { useLocalSearchParams, useRouter } from "expo-router";
import { LivePlayer, phaseLabel, type Phase } from "@/live/LivePlayer";
import { space } from "@/styles/theme";

export default function LiveFull() {
  const { ip, name, hd: hdParam } = useLocalSearchParams<{ ip: string; name?: string; hd?: string }>();
  const [hd, setHd] = useState(hdParam === "1");
  const [phase, setPhase] = useState<Phase>("connecting");
  const [retried, setRetried] = useState(false);
  // Controls fade out so the picture is the whole screen; a tap anywhere brings them back. Nothing
  // auto-hides while the stream is still connecting — hiding a "Connecting…" badge would leave a
  // black rectangle with no explanation.
  const [chrome, setChrome] = useState(true);
  const router = useRouter();

  useEffect(() => {
    ScreenOrientation.lockAsync(ScreenOrientation.OrientationLock.LANDSCAPE);
    return () => {
      // Restore on the way out, including on a swipe-back or a crash of this screen — the rest of
      // the app is portrait-only by design.
      ScreenOrientation.lockAsync(ScreenOrientation.OrientationLock.PORTRAIT_UP);
    };
  }, []);

  useEffect(() => {
    if (!chrome || phase !== "live") return;
    const timer = setTimeout(() => setChrome(false), 3000);
    return () => clearTimeout(timer);
  }, [chrome, phase]);

  if (!ip) return null;

  return (
    <View style={styles.root}>
      <StatusBar hidden />
      <LivePlayer
        ip={ip}
        hd={hd}
        onPhase={(p) => {
          setPhase(p);
          if (p === "connecting") setRetried(true);
        }}
      />

      {/* The whole picture is the tap target for showing controls again — a small hit area on a
          screen someone is holding sideways is a control nobody finds. */}
      <Pressable style={StyleSheet.absoluteFill} onPress={() => setChrome((v) => !v)} />

      {chrome ? (
        <>
          <View style={[styles.pill, styles.badge]}>
            <Text style={styles.pillText}>{phaseLabel(phase, retried)}</Text>
          </View>
          {name ? (
            <View style={[styles.pill, styles.name]}>
              <Text style={styles.pillText}>{name}</Text>
            </View>
          ) : null}
          <Pressable style={[styles.pill, styles.quality]} onPress={() => setHd((v) => !v)}>
            <Text style={styles.pillText}>{hd ? "HD" : "SD"}</Text>
          </Pressable>
          <Pressable style={[styles.pill, styles.close]} onPress={() => router.back()}>
            <Text style={styles.pillText}>✕</Text>
          </Pressable>
        </>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create({
  root: { flex: 1, backgroundColor: "#000" },
  pill: {
    position: "absolute",
    backgroundColor: "#000000AA",
    borderRadius: 999,
    paddingHorizontal: space.md,
    paddingVertical: space.xs,
  },
  pillText: { color: "#FFF", fontSize: 12, fontWeight: "600" },
  badge: { top: space.md, left: space.lg },
  name: { top: space.md, left: space.lg + 96 },
  quality: { top: space.md, right: space.lg + 44 },
  close: { top: space.md, right: space.lg },
});
