// Recordings — Phase 3. Metadata from control, playback from the presigned R2 URL.
//
// Clips are two-tier: the video lives on the edge and a durable copy goes to R2. Only an
// `uploaded` clip has a cloud URL, so the list says plainly when one is not playable yet instead of
// offering a button that 409s.
import { useEffect, useState } from "react";
import { ActivityIndicator, Pressable, ScrollView, StyleSheet, View } from "react-native";
import { useVideoPlayer, VideoView } from "expo-video";
import { formatBytes, formatDuration, listRecordings, recordingUrl, type Recording } from "@/data";
import { Danger, Heading, Muted, Screen, Strong } from "@/styles/ui";
import { radius, space, useTheme } from "@/styles/theme";

export default function Recordings() {
  const [items, setItems] = useState<Recording[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [playing, setPlaying] = useState<{ id: number; url: string } | null>(null);
  const [opening, setOpening] = useState<number | null>(null);
  const t = useTheme();

  const player = useVideoPlayer(playing?.url ?? null, (p) => {
    p.loop = false;
    p.play();
  });

  useEffect(() => {
    listRecordings()
      .then((page) => setItems(page.items))
      .catch((e) => setError((e as Error).message));
  }, []);

  async function open(r: Recording) {
    setOpening(r.id);
    setError(null);
    try {
      const { url } = await recordingUrl(r.id);
      setPlaying({ id: r.id, url });
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setOpening(null);
    }
  }

  function stateLabel(r: Recording): string {
    if (r.deleting) return "Deleting…";
    if (r.upload_state === "uploaded") return "In the cloud";
    if (r.upload_state === "uploading") return "Uploading…";
    if (r.upload_state === "failed") return "Upload failed";
    return "On the box only";
  }

  const playable = (r: Recording) => r.upload_state === "uploaded" && !r.deleting;

  if (error && !items) {
    return (
      <Screen style={{ padding: space.xl, justifyContent: "center", gap: space.md }}>
        <Heading>Recordings</Heading>
        <Danger>{error}</Danger>
        <Muted>Recordings need the recordings.read permission.</Muted>
      </Screen>
    );
  }

  if (!items) {
    return (
      <Screen style={{ alignItems: "center", justifyContent: "center" }}>
        <ActivityIndicator color={t.mutedForeground} />
      </Screen>
    );
  }

  return (
    <Screen>
      {playing ? (
        <View style={{ aspectRatio: 16 / 9, backgroundColor: "#000" }}>
          <VideoView style={{ flex: 1 }} player={player} contentFit="contain" nativeControls fullscreenOptions={{ enable: true }} />
        </View>
      ) : null}

      <ScrollView contentContainerStyle={{ padding: space.lg, gap: space.sm, paddingBottom: space.xxl }}>
        <Heading>Recordings</Heading>
        {error ? <Danger>{error}</Danger> : null}
        {items.length === 0 ? (
          <Muted>No clips yet. A workflow with a Record node creates them.</Muted>
        ) : (
          items.map((r) => (
            <Pressable
              key={r.id}
              onPress={() => playable(r) && open(r)}
              disabled={!playable(r)}
              style={{
                backgroundColor: playing?.id === r.id ? t.elev7 : t.card,
                borderColor: t.border,
                borderWidth: StyleSheet.hairlineWidth,
                borderRadius: radius.lg,
                padding: space.md,
                gap: 2,
                opacity: playable(r) ? 1 : 0.6,
              }}
            >
              <Strong>{r.camera_name || r.camera_ip}</Strong>
              <Muted>
                {new Date(r.started_at).toLocaleString()} · {formatDuration(r.duration_secs)} ·{" "}
                {formatBytes(r.size_bytes)}
              </Muted>
              <Muted>{opening === r.id ? "Opening…" : stateLabel(r)}</Muted>
            </Pressable>
          ))
        )}
      </ScrollView>
    </Screen>
  );
}
