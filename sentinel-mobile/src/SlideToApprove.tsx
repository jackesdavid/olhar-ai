// Slide to approve (card #176, ticket 06).
//
// A gate opens a door, arms an alarm, unlocks a thing. A button can be tapped by a pocket; a slide
// cannot. The distance is the safety, so it is deliberately most of the width — but it HOLDS at the
// end rather than snapping back, because someone in a hurry should not have to be precise, and a
// slide that springs away the moment a thumb lifts is a slide people fight.
//
// Built on `PanResponder` + `Animated`, both React Native core: this screen must not wait on a
// gesture library to exist in the app.
import { useRef, useState } from "react";
import { Animated, PanResponder, StyleSheet, Text, View } from "react-native";
import { radius, space, typeScale, useTheme } from "./styles/theme";

const KNOB = 56;
const TRACK_H = 64;
/** How far along the track counts as "they meant it". Not 100%: the last few pixels are where a
 *  thumb runs out of screen, and demanding them turns a decisive gesture into a fiddly one. */
const COMMIT = 0.92;

export function SlideToApprove({
  label,
  confirmingLabel,
  disabled,
  busy,
  onCommit,
}: {
  label: string;
  confirmingLabel: string;
  disabled?: boolean;
  busy?: boolean;
  onCommit: () => void;
}) {
  const t = useTheme();
  const x = useRef(new Animated.Value(0)).current;
  const [trackW, setTrackW] = useState(0);
  const committed = useRef(false);
  const travel = Math.max(0, trackW - KNOB - 8);

  const responder = useRef(
    PanResponder.create({
      onStartShouldSetPanResponder: () => true,
      onMoveShouldSetPanResponder: (_e, g) => Math.abs(g.dx) > 2,
      onPanResponderMove: (_e, g) => {
        if (committed.current || disabled || busy) return;
        x.setValue(Math.max(0, Math.min(travel, g.dx)));
      },
      onPanResponderRelease: (_e, g) => {
        if (committed.current || disabled || busy) return;
        const reached = travel > 0 && g.dx >= travel * COMMIT;
        if (reached) {
          committed.current = true;
          // Settle AT the end and stay there — the knob holding is the visual promise that the
          // answer was sent, and it must not rewind while the request is in flight.
          Animated.timing(x, { toValue: travel, duration: 90, useNativeDriver: false }).start();
          onCommit();
          return;
        }
        Animated.spring(x, { toValue: 0, useNativeDriver: false, bounciness: 0 }).start();
      },
      onPanResponderTerminate: () => {
        if (!committed.current) {
          Animated.spring(x, { toValue: 0, useNativeDriver: false, bounciness: 0 }).start();
        }
      },
    }),
  ).current;

  const dim = disabled ? 0.4 : 1;
  return (
    <View
      onLayout={(e) => setTrackW(e.nativeEvent.layout.width)}
      style={[
        styles.track,
        { backgroundColor: t.elev1, borderColor: t.border, borderRadius: radius.xl, opacity: dim },
      ]}
    >
      <Text style={[typeScale.body, styles.hint, { color: t.mutedForeground }]}>
        {busy ? confirmingLabel : label}
      </Text>
      <Animated.View
        {...responder.panHandlers}
        style={[
          styles.knob,
          {
            backgroundColor: t.foreground,
            borderRadius: radius.xl,
            transform: [{ translateX: x }],
          },
        ]}
      >
        <Text style={[styles.chevron, { color: t.background }]}>›››</Text>
      </Animated.View>
    </View>
  );
}

const styles = StyleSheet.create({
  track: {
    height: TRACK_H,
    borderWidth: StyleSheet.hairlineWidth,
    justifyContent: "center",
    padding: 4,
  },
  hint: { position: "absolute", width: "100%", textAlign: "center" },
  knob: {
    width: KNOB,
    height: TRACK_H - 8,
    alignItems: "center",
    justifyContent: "center",
  },
  chevron: { fontSize: 18, fontWeight: "700", letterSpacing: -1 },
});

export const SLIDE_SPACING = space.md;
