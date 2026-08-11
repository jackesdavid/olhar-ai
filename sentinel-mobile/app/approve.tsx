// The approval screen (card #176, ticket 06).
//
// Someone's phone buzzes and they have to decide something, possibly in a hurry, possibly on a lock
// screen in a corridor. So the screen is arranged by what they need in what order: which automation
// is asking (small — context, not the question), what it wants (large — the question), how long they
// have (a countdown that actually ticks), and one gesture that cannot mis-fire.
//
// It is equally a screen about the states that are NOT "answer me". A request that was already
// answered, one that expired while the phone was in a pocket, an unknown id from a stale push, no
// connectivity — each has to say what happened plainly, because the alternative is a person tapping
// a dead slider wondering whether they just opened their front door.
import { useCallback, useEffect, useRef, useState } from "react";
import { ActivityIndicator, AppState, Pressable, ScrollView, StyleSheet, View } from "react-native";
import { useLocalSearchParams, useRouter } from "expo-router";
import {
  answer,
  confirmPresence,
  formatLeft,
  listPending,
  secondsLeft,
  type PendingRequest,
} from "@/deviceGate";
import { SlideToApprove } from "@/SlideToApprove";
import { Body, Card, Danger, Heading, Muted, Screen, Strong, Title } from "@/styles/ui";
import { space, typeScale, useTheme } from "@/styles/theme";

/** Everything the screen can be. `answered` and `expired` are outcomes, not errors — they are what
 *  a duplicate push and a face-down phone look like from here, and both are normal. */
type Phase =
  | { k: "loading" }
  | { k: "none" }
  | { k: "asking"; req: PendingRequest }
  | { k: "sending"; req: PendingRequest }
  | { k: "answered"; approved: boolean }
  | { k: "expired" }
  | { k: "settled" }
  | { k: "offline"; reason: string };

export default function Approve() {
  // The push carries both: `request_id` says WHICH, `token` proves it. The token never reaches the
  // pending list — it exists in the notification payload and nowhere else the phone can read.
  const { request_id, token } = useLocalSearchParams<{ request_id?: string; token?: string }>();
  const [phase, setPhase] = useState<Phase>({ k: "loading" });
  const [now, setNow] = useState(Date.now());
  const router = useRouter();
  const t = useTheme();

  // One second, locally. The server owns the deadline; this only renders it.
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, []);

  const load = useCallback(async () => {
    try {
      const pending = await listPending();
      // Prefer the request the push named; fall back to whatever is waiting, so opening the app
      // cold (no push tapped) still shows the thing that needs a person.
      const req = request_id ? pending.find((p) => p.id === request_id) : pending[0];
      if (!req) {
        // The push named a request the server no longer lists: answered by another session, or
        // expired. Both are "there is nothing for you to do", said honestly.
        setPhase(request_id ? { k: "settled" } : { k: "none" });
        return;
      }
      setPhase(secondsLeft(req.expires_at) <= 0 ? { k: "expired" } : { k: "asking", req });
    } catch (e) {
      setPhase({ k: "offline", reason: (e as Error).message || "No connection." });
    }
  }, [request_id]);

  useEffect(() => {
    load();
    // Re-check on foreground: the pending list is the reliable path (decision 05), and a phone that
    // sat in a pocket may be holding a request that has since been answered elsewhere.
    const sub = AppState.addEventListener("change", (s) => {
      if (s === "active") load();
    });
    return () => sub.remove();
  }, [load]);

  // Expiry while the screen is open. The graph has already taken `failure` by now — pretending the
  // slider still works would be a lie the person acts on.
  const asking = phase.k === "asking" ? phase.req : null;
  const left = asking ? secondsLeft(asking.expires_at, now) : 0;
  useEffect(() => {
    if (asking && left <= 0) setPhase({ k: "expired" });
  }, [asking, left]);

  const busy = useRef(false);
  async function decide(approved: boolean) {
    if (busy.current || phase.k !== "asking") return;
    const req = phase.req;
    if (!token) {
      // No token means this screen was opened without the push that carries it. The pending list
      // deliberately does not return one, so there is nothing to fall back to.
      setPhase({ k: "offline", reason: "Open this from the notification — the approval code is in it." });
      return;
    }
    busy.current = true;
    setPhase({ k: "sending", req });
    try {
      // The biometric comes AFTER the gesture, not before: the slide is the intent, the face is the
      // proof. Asking for a face first would make people authenticate to read the question.
      const who = await confirmPresence(
        approved ? `Approve: ${req.prompt}` : `Deny: ${req.prompt}`,
      );
      if (!who.ok) {
        setPhase({ k: "asking", req }); // cancelled — back to the question, nothing sent
        return;
      }
      const out = await answer(req.id, token, approved, who.factor);
      if (out.ok) setPhase({ k: "answered", approved });
      else if (out.settled) setPhase({ k: "settled" });
      else setPhase({ k: "offline", reason: out.reason });
    } finally {
      busy.current = false;
    }
  }

  return (
    <Screen>
      <ScrollView contentContainerStyle={styles.body}>
        {phase.k === "loading" && <ActivityIndicator />}

        {(phase.k === "asking" || phase.k === "sending") && (
          <Card>
            <Muted>{phase.req.flow_name}</Muted>
            <Title>{phase.req.prompt}</Title>
            <Body>
              <Strong>{formatLeft(left)}</Strong> left to decide
            </Body>

            <SlideToApprove
              label="Slide to approve"
              confirmingLabel="Confirming…"
              busy={phase.k === "sending"}
              onCommit={() => decide(true)}
            />

            {/* Deny is an ordinary button and asks nothing extra. A confirmation here would be
                friction on the SAFE direction, and the arm it fires is an answer either way — the
                graph learns someone said no, which is not the same as nobody deciding. */}
            <Pressable
              onPress={() => decide(false)}
              disabled={phase.k === "sending"}
              style={({ pressed }) => [styles.deny, { opacity: pressed ? 0.6 : 1 }]}
            >
              <Body>Deny</Body>
            </Pressable>
          </Card>
        )}

        {phase.k === "answered" && (
          <Card>
            <Heading>{phase.approved ? "Approved" : "Denied"}</Heading>
            <Muted>The automation has been told. You can close this.</Muted>
          </Card>
        )}

        {phase.k === "expired" && (
          <Card>
            <Heading>Time is up</Heading>
            <Muted>
              Nobody answered in time, so the automation took its failure path. Nothing was approved.
            </Muted>
          </Card>
        )}

        {phase.k === "settled" && (
          <Card>
            <Heading>Already answered</Heading>
            <Muted>This request was decided somewhere else, or it expired. Nothing to do.</Muted>
          </Card>
        )}

        {phase.k === "none" && (
          <Card>
            <Heading>Nothing waiting</Heading>
            <Muted>
              When an automation needs your approval it will appear here, and your phone will buzz.
            </Muted>
          </Card>
        )}

        {phase.k === "offline" && (
          <Card>
            <Heading>Couldn&apos;t reach the server</Heading>
            <Danger>{phase.reason}</Danger>
            <Muted>
              Nothing was sent. The request is still waiting on the server until its time runs out.
            </Muted>
            <Pressable onPress={load} style={styles.deny}>
              <Body>Try again</Body>
            </Pressable>
          </Card>
        )}

        {phase.k !== "asking" && phase.k !== "sending" && phase.k !== "loading" && (
          <Pressable onPress={() => router.back()} style={styles.deny}>
            <Muted>Close</Muted>
          </Pressable>
        )}
      </ScrollView>
    </Screen>
  );
}

const styles = StyleSheet.create({
  body: { padding: space.lg, gap: space.lg },
  deny: { alignSelf: "center", paddingVertical: space.md, paddingHorizontal: space.lg },
});
