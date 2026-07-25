// Capture settings (card #143) — makes the face-capture parameters user-tunable instead of the
// hardcoded constants they were. Reached from the Capture tab; saving persists and returns.
import { useEffect, useState } from "react";
import { Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { useRouter } from "expo-router";

import {
  DEFAULT_SETTINGS,
  INTERVAL_OPTIONS,
  QUALITY_OPTIONS,
  RESOLUTION_OPTIONS,
  loadSettings,
  saveSettings,
  type CaptureSettings,
} from "@/captureSettings";
import { Button, Card, Heading, Muted, Strong } from "@/styles/ui";
import { radius, space, useTheme } from "@/styles/theme";

const MODE_OPTIONS = [
  { label: "Continuous", value: "continuous" as const },
  { label: "Tap to snap", value: "manual" as const },
];
const CAMERA_OPTIONS = [
  { label: "Front", value: "front" as const },
  { label: "Back", value: "back" as const },
];

export default function CaptureSettingsScreen() {
  const router = useRouter();
  const [s, setS] = useState<CaptureSettings>(DEFAULT_SETTINGS);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    loadSettings().then(setS);
  }, []);

  async function save() {
    setSaving(true);
    await saveSettings(s);
    setSaving(false);
    router.back();
  }

  return (
    <ScrollView contentContainerStyle={styles.body}>
      <Heading>Capture settings</Heading>
      <Muted>How the phone captures faces while the Capture screen is open.</Muted>

      <Card>
        <Field label="Mode">
          <Segmented options={MODE_OPTIONS} value={s.mode} onChange={(mode) => setS({ ...s, mode })} />
        </Field>
        {s.mode === "continuous" ? (
          <Field label="Interval" hint="How often a photo is taken.">
            <Segmented
              options={INTERVAL_OPTIONS}
              value={s.intervalMs}
              onChange={(intervalMs) => setS({ ...s, intervalMs })}
            />
          </Field>
        ) : null}
      </Card>

      <Card>
        <Field label="Resolution" hint="Longest edge of the uploaded image.">
          <Segmented
            options={RESOLUTION_OPTIONS}
            value={s.maxWidth}
            onChange={(maxWidth) => setS({ ...s, maxWidth })}
          />
        </Field>
        <Field label="Quality" hint="Higher is clearer but larger over cellular.">
          <Segmented
            options={QUALITY_OPTIONS}
            value={s.quality}
            onChange={(quality) => setS({ ...s, quality })}
          />
        </Field>
        <Field label="Camera">
          <Segmented options={CAMERA_OPTIONS} value={s.facing} onChange={(facing) => setS({ ...s, facing })} />
        </Field>
      </Card>

      <Button label="Save" onPress={save} busy={saving} />
    </ScrollView>
  );
}

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <View style={{ gap: space.sm }}>
      <Strong>{label}</Strong>
      {hint ? <Muted>{hint}</Muted> : null}
      {children}
    </View>
  );
}

/** A row of mutually-exclusive options — the app's slider-free numeric/enum picker. Generic over the
 *  option value so mode / interval / resolution / quality / camera all share one control. */
function Segmented<T extends string | number>({
  options,
  value,
  onChange,
}: {
  options: { label: string; value: T }[];
  value: T;
  onChange: (v: T) => void;
}) {
  const t = useTheme();
  return (
    <View style={[styles.segment, { borderColor: t.border, borderRadius: radius.md }]}>
      {options.map((o) => {
        const on = o.value === value;
        return (
          <Pressable
            key={String(o.value)}
            onPress={() => onChange(o.value)}
            style={[styles.segmentItem, on && { backgroundColor: t.foreground }]}
          >
            <Text style={{ color: on ? t.background : t.foreground, fontWeight: on ? "600" : "400" }}>
              {o.label}
            </Text>
          </Pressable>
        );
      })}
    </View>
  );
}

const styles = StyleSheet.create({
  body: { padding: space.lg, gap: space.lg },
  segment: { flexDirection: "row", borderWidth: StyleSheet.hairlineWidth, overflow: "hidden" },
  segmentItem: { flex: 1, paddingVertical: space.md, alignItems: "center" },
});
