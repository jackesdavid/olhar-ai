// Slide to approve (card #176, ticket 06).
//
// A gate opens a door, arms an alarm, unlocks a thing. A button can be tapped by a pocket; a slide
// cannot. The distance is the safety, so it is deliberately most of the width — but it HOLDS at the
// end rather than snapping back, because someone in a hurry should not have to be precise, and a
// slide that springs away the moment a thumb lifts is a slide people fight.
//
// Built on `PanResponder` + `Animated`, both React Native core: this screen must not wait on a
// gesture library to exist in the app.
import { useEffect, useRef, useState } from "react";
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
  onDragging,
}: {
  label: string;
  confirmingLabel: string;
  disabled?: boolean;
  busy?: boolean;
  onCommit: () => void;
  /** Fires true while a finger is on the knob. The parent uses it to FREEZE its ScrollView.
   *
   *  Claiming the pan responder is not enough on its own: the ScrollView's native recogniser keeps
   *  running underneath, so the page slides while the knob does and the gesture is lost. The only
   *  reliable answer is for the scroll view to stop scrolling while a slide is in progress. */
  onDragging?: (active: boolean) => void;
}) {
  const t = useTheme();
  const x = useRef(new Animated.Value(0)).current;
  const [trackW, setTrackW] = useState(0);
  const committed = useRef(false);

  // Everything the gesture reads lives in a REF, not a closure variable.
  //
  // `PanResponder.create` runs ONCE (it is wrapped in `useRef`), so it captures whatever the render
  // scope held at that moment. The track has not been laid out yet then, so `travel` was 0 — and
  // `Math.min(0, dx)` pinned the knob at zero forever. The slider looked alive and could not be
  // moved a pixel. Same trap for `disabled`/`busy`/`onCommit`: a stale closure would keep answering
  // with the first render's values.
  const travelRef = useRef(0);
  const lockedRef = useRef(false);
  const commitRef = useRef(onCommit);
  const draggingRef = useRef(onDragging);
  useEffect(() => {
    lockedRef.current = !!disabled || !!busy;
  }, [disabled, busy]);
  useEffect(() => {
    commitRef.current = onCommit;
  }, [onCommit]);
  useEffect(() => {
    draggingRef.current = onDragging;
  }, [onDragging]);

  const responder = useRef(
    PanResponder.create({
      // CLAIM ON TOUCH-DOWN, not on movement. A finger on the knob is a slide, full stop — the
      // parent ScrollView must never get a say. Deciding by direction (|dx| > |dy|) meant a slide
      // with any vertical drift handed the gesture to the scroll view part-way, which terminated
      // the responder and sprang the knob back to zero. Reported as exactly that: scroll a little
      // and the slide position is lost.
      onStartShouldSetPanResponder: () => true,
      onStartShouldSetPanResponderCapture: () => true,
      onMoveShouldSetPanResponder: () => true,
      onMoveShouldSetPanResponderCapture: () => true,
      onPanResponderTerminationRequest: () => false,
      onPanResponderGrant: () => {
        if (!committed.current && !lockedRef.current) draggingRef.current?.(true);
      },
      onPanResponderMove: (_e, g) => {
        if (committed.current || lockedRef.current) return;
        x.setValue(Math.max(0, Math.min(travelRef.current, g.dx)));
      },
      onPanResponderRelease: (_e, g) => {
        draggingRef.current?.(false);
        if (committed.current || lockedRef.current) return;
        const travel = travelRef.current;
        const reached = travel > 0 && g.dx >= travel * COMMIT;
        if (reached) {
          committed.current = true;
          // Settle AT the end and stay there — the knob holding is the visual promise that the
          // answer was sent, and it must not rewind while the request is in flight.
          Animated.timing(x, { toValue: travel, duration: 90, useNativeDriver: false }).start();
          commitRef.current();
          return;
        }
        Animated.spring(x, { toValue: 0, useNativeDriver: false, bounciness: 0 }).start();
      },
      // If the system takes the gesture anyway (a call, a system sheet), HOLD the knob where it
      // is rather than rewinding. Rewinding punishes someone for an interruption they did not cause.
      onPanResponderTerminate: () => {
        draggingRef.current?.(false);
        if (!committed.current) {
          Animated.spring(x, { toValue: 0, useNativeDriver: false, bounciness: 0 }).start();
        }
      },
    }),
  ).current;

  const dim = disabled ? 0.4 : 1;
  return (
    <View
      onLayout={(e) => {
        const w = e.nativeEvent.layout.width;
        setTrackW(w);
        travelRef.current = Math.max(0, w - KNOB - 8);
      }}
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
