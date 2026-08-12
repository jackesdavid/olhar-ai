// Charts, drawn with Views.
//
// A week of activity is seven numbers. Pulling in a charting library to draw seven rectangles would
// cost a native dependency, a bundle, and a second design language — the ink theme has one colour
// and these are bars of it. `react-native-svg` is not a dependency here for the same reason the
// mark is not.
//
// Colour is not carrying meaning: every bar is the same ink, and HEIGHT is the whole message.
// Today is the only one that reads differently, because "is today busy?" is the question someone
// opens a dashboard to answer.
import { StyleSheet, Text, View } from "react-native";
import { radius, space, typeScale, useTheme } from "./theme";

export type Bar = { label: string; count: number };

export function BarChart({
  bars,
  peak,
  height = 84,
}: {
  bars: Bar[];
  peak: number;
  height?: number;
}) {
  const t = useTheme();
  return (
    <View style={[styles.row, { height: height + 22 }]}>
      {bars.map((b, i) => {
        const last = i === bars.length - 1;
        // A day with nothing still gets a sliver, so the axis reads as a week rather than a gap.
        const h = Math.max(3, Math.round((b.count / peak) * height));
        return (
          <View key={b.label + i} style={styles.col}>
            <View style={styles.barSlot}>
              <View
                style={{
                  height: h,
                  width: "100%",
                  borderRadius: radius.sm,
                  backgroundColor: t.foreground,
                  // Today is solid; the days behind it recede. One focal point, even in a chart.
                  opacity: last ? 1 : 0.28,
                }}
              />
            </View>
            <Text
              style={[
                typeScale.caption,
                { color: last ? t.foreground : t.mutedForeground, fontWeight: last ? "600" : "400" },
              ]}
            >
              {b.label}
            </Text>
          </View>
        );
      })}
    </View>
  );
}

/** One number and what it counts. The number leads; the label explains it. */
export function Stat({ value, label }: { value: number | string; label: string }) {
  const t = useTheme();
  return (
    <View style={styles.stat}>
      <Text style={[styles.statValue, { color: t.foreground }]}>{value}</Text>
      <Text style={[typeScale.caption, { color: t.mutedForeground }]}>{label}</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  row: { flexDirection: "row", alignItems: "flex-end", gap: space.sm },
  col: { flex: 1, alignItems: "center", gap: 6 },
  barSlot: { flex: 1, width: "100%", justifyContent: "flex-end" },
  stat: { gap: 2 },
  // `tabular-nums` so a counter that ticks does not jitter its own label sideways.
  statValue: { fontSize: 26, fontWeight: "700", letterSpacing: -0.5, fontVariant: ["tabular-nums"] },
});
