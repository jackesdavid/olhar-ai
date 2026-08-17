// The live video surface, shared by the Live tab and the fullscreen view.
//
// ONE component on purpose. The reconnect policy, the opaque-origin workaround and the remount-key
// trick are subtle enough that a second copy would drift within a week — and the fullscreen view is
// exactly where a stale copy would be noticed last.
import { useEffect, useMemo, useState } from "react";
import { StyleSheet, View } from "react-native";
import { WebView } from "react-native-webview";
import { GATEWAY_BASE } from "@/config";
import { streamName } from "@/data";
import { currentToken } from "@/session";
import { playerHtml, type PlayerEvent } from "@/live/msePlayer";

export type Phase = "connecting" | "live" | "stalled" | "error";

export function LivePlayer({
  ip,
  hd,
  onPhase,
  onError,
  style,
}: {
  ip: string;
  hd: boolean;
  onPhase?: (p: Phase) => void;
  onError?: (m: string | null) => void;
  style?: object;
}) {
  const [phase, setPhase] = useState<Phase>("connecting");
  /** Bumped to remount the WebView, which is how a stream restarts: a fresh socket and a fresh
   *  MediaSource. Reusing one that has already errored does not work. */
  const [attempt, setAttempt] = useState(0);

  const token = currentToken();
  const wsBase = GATEWAY_BASE.replace(/^http/, "ws");
  const html = useMemo(
    () => (token ? playerHtml({ wsBase, stream: streamName(ip, hd), token }) : null),
    [ip, hd, token, wsBase],
  );

  // A new camera or quality is a new stream — start its reconnect count over, or a switch inherits
  // the previous camera's backoff and looks slow for no reason.
  useEffect(() => {
    setAttempt(0);
    setPhase("connecting");
    onPhase?.("connecting");
    onError?.(null);
  }, [ip, hd]);

  // A live stream WILL drop — a decode error, a socket close, the box restarting. What matters is
  // that it returns on its own: nobody checking a camera should have to know to leave the screen
  // and come back. Backing off a little so a camera that is genuinely down does not spin.
  useEffect(() => {
    if (phase !== "error" && phase !== "stalled") return;
    const delay = Math.min(1500 * (attempt + 1), 8000);
    const timer = setTimeout(() => {
      onError?.(null);
      setPhase("connecting");
      onPhase?.("connecting");
      setAttempt((n) => n + 1);
    }, delay);
    return () => clearTimeout(timer);
  }, [phase, attempt]);

  function onMessage(raw: string) {
    let msg: PlayerEvent;
    try {
      msg = JSON.parse(raw);
    } catch {
      return;
    }
    if (msg.type === "phase") {
      setPhase(msg.phase);
      onPhase?.(msg.phase);
    }
    if (msg.type === "error") {
      setPhase("error");
      onPhase?.("error");
      onError?.(msg.message);
    }
  }

  if (!html) return <View style={[styles.black, style]} />;
  return (
    <WebView
      key={`${ip}-${hd ? "hd" : "sd"}-${attempt}`}
      // Inline HTML loads as an opaque origin, and WKWebView blocks a WebSocket from one. Giving
      // the page the gateway's own origin makes the connection same-origin.
      source={{ html, baseUrl: GATEWAY_BASE }}
      originWhitelist={["*"]}
      style={[styles.black, style]}
      allowsInlineMediaPlayback
      mediaPlaybackRequiresUserAction={false}
      onMessage={(e) => onMessage(e.nativeEvent.data)}
    />
  );
}

/** What the badge over the video should say, given the phase and whether we have retried. */
export function phaseLabel(phase: Phase, retried: boolean): string {
  if (phase === "live") return "LIVE";
  if (phase === "error" || phase === "stalled") return "Reconnecting…";
  return retried ? "Reconnecting…" : "Connecting…";
}

const styles = StyleSheet.create({
  black: { flex: 1, backgroundColor: "#000" },
});
