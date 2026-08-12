// The Olhar AI mark, in React Native.
//
// An eclipse: a filled disc with a crescent bitten out of its upper right, the bite carrying a small
// dot, and a satellite dot further out — an eye mid-blink, or a sun mid-eclipse. A dotted ring holds
// the whole thing. Traced from `assets/olhar-ai-mark-white.svg`, the same geometry the app icon and
// the web wordmark use, so all three read as one mark rather than three interpretations.
//
// Drawn with plain Views. `react-native-svg` is not a dependency and a circle is a View with
// `borderRadius`; the only trick is the crescent, which is a disc of the BACKGROUND colour laid over
// the white one. That is why `bg` matters: the bite has to match whatever surface the mark sits on.
import { useEffect, useRef } from "react";
import { Animated, Easing, StyleSheet, View, type ViewStyle } from "react-native";
import { useTheme } from "./theme";

/** Geometry in the SVG's 120×120 space, so the numbers below match the source file exactly. */
const BOX = 120;
const RING_R = 50;
const DISC_R = 31;
const BITE = { x: 71, y: 49, r: 27 };
const PUPIL = { x: 71, y: 49, r: 6 };
const SATELLITE = { x: 95.4, y: 24.6, r: 4 };

/**
 * The mark.
 *
 * `bg` must be the colour behind it — the crescent is subtractive, and a bite in the wrong colour
 * is a grey smear across the eye. Defaults to the page canvas.
 */
export function OlharMark({
  size = 48,
  color,
  bg,
  ring = true,
  style,
}: {
  size?: number;
  color?: string;
  bg?: string;
  /** The dotted orbit. Off for small sizes, where it collapses into a fuzzy line. */
  ring?: boolean;
  style?: ViewStyle;
}) {
  const t = useTheme();
  const ink = color ?? t.foreground;
  const behind = bg ?? t.background;
  const k = size / BOX; // SVG units → px
  const circle = (cx: number, cy: number, r: number, fill: string, extra?: ViewStyle): ViewStyle => ({
    position: "absolute",
    left: (cx - r) * k,
    top: (cy - r) * k,
    width: r * 2 * k,
    height: r * 2 * k,
    borderRadius: r * k,
    backgroundColor: fill,
    ...extra,
  });

  return (
    <View style={[{ width: size, height: size }, style]}>
      {ring ? (
        <View
          style={[
            circle(BOX / 2, BOX / 2, RING_R, "transparent"),
            {
              borderWidth: Math.max(1, 2 * k),
              borderColor: ink,
              // The SVG's dash array is a dotted orbit; RN gives us `dotted` borders, which is the
              // same read at this size without pulling in a drawing library.
              borderStyle: "dotted",
              opacity: 0.3,
            },
          ]}
        />
      ) : null}
      <View style={circle(BOX / 2, BOX / 2, DISC_R, ink)} />
      {/* The eclipse. Subtractive: it paints the surface colour back over the disc. */}
      <View style={circle(BITE.x, BITE.y, BITE.r, behind)} />
      <View style={circle(PUPIL.x, PUPIL.y, PUPIL.r, ink)} />
      <View style={circle(SATELLITE.x, SATELLITE.y, SATELLITE.r, ink)} />
    </View>
  );
}

/**
 * The loader: the eclipse closing and opening.
 *
 * The mark's own motion, not a borrowed spinner — the bite drifts across the disc the way an eye
 * blinks. Slow (1.4s) so it reads as attention rather than anxiety.
 */
export function Loader({ size = 44, bg }: { size?: number; bg?: string }) {
  const t = useTheme();
  const anim = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    const loop = Animated.loop(
      Animated.sequence([
        Animated.timing(anim, { toValue: 1, duration: 700, easing: Easing.inOut(Easing.quad), useNativeDriver: true }),
        Animated.timing(anim, { toValue: 0, duration: 700, easing: Easing.inOut(Easing.quad), useNativeDriver: true }),
      ]),
    );
    loop.start();
    return () => loop.stop();
  }, [anim]);

  return (
    <Animated.View
      style={{
        opacity: anim.interpolate({ inputRange: [0, 1], outputRange: [0.55, 1] }),
        transform: [{ scale: anim.interpolate({ inputRange: [0, 1], outputRange: [0.95, 1] }) }],
      }}
    >
      <OlharMark size={size} bg={bg ?? t.background} />
    </Animated.View>
  );
}

/**
 * Live state: a dot with a ring radiating from it.
 *
 * Colour is not what says "live" — the ink theme has no brand green. A static dot means "known
 * state", a pulsing one means "right now".
 */
export function PulseDot({ live = true, size = 8 }: { live?: boolean; size?: number }) {
  const t = useTheme();
  const ping = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    if (!live) return;
    const loop = Animated.loop(
      Animated.timing(ping, { toValue: 1, duration: 1800, easing: Easing.out(Easing.quad), useNativeDriver: true }),
    );
    loop.start();
    return () => loop.stop();
  }, [live, ping]);

  return (
    <View style={{ width: size * 3, height: size * 3, alignItems: "center", justifyContent: "center" }}>
      {live ? (
        <Animated.View
          style={{
            position: "absolute",
            width: size,
            height: size,
            borderRadius: size / 2,
            borderWidth: 1,
            borderColor: t.foreground,
            opacity: ping.interpolate({ inputRange: [0, 1], outputRange: [0.5, 0] }),
            transform: [{ scale: ping.interpolate({ inputRange: [0, 1], outputRange: [1, 2.8] }) }],
          }}
        />
      ) : null}
      <View
        style={{
          width: size,
          height: size,
          borderRadius: size / 2,
          backgroundColor: live ? t.foreground : t.mutedForeground,
          opacity: live ? 1 : 0.5,
        }}
      />
    </View>
  );
}

/** Empty state: the mark, one line that says what to do, and room to breathe. */
export function EmptyState({
  title,
  children,
  action,
  bg,
}: {
  title: string;
  children?: React.ReactNode;
  action?: React.ReactNode;
  bg?: string;
}) {
  const t = useTheme();
  return (
    <View style={styles.empty}>
      <OlharMark size={64} bg={bg} />
      <View style={{ gap: 4, alignItems: "center" }}>
        <Animated.Text style={{ fontSize: 17, fontWeight: "600", color: t.foreground }}>
          {title}
        </Animated.Text>
        {children ? (
          <Animated.Text style={{ fontSize: 14, color: t.mutedForeground, textAlign: "center" }}>
            {children}
          </Animated.Text>
        ) : null}
      </View>
      {action}
    </View>
  );
}

const styles = StyleSheet.create({
  empty: { alignItems: "center", gap: 16, paddingVertical: 40, paddingHorizontal: 24 },
});
