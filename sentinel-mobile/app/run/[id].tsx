// `olharai://run/<graph_id>` — what a home-screen widget button opens (card #178).
//
// This is a ROUTE, not a listener, because expo-router owns incoming URLs: it maps a deep link onto
// the file tree and shows "Unmatched Route" for anything it cannot resolve. A `Linking` handler in
// the root layout never gets the chance — the router has already decided the URL is a page and
// failed to find one. The widget's tap ended on that screen until this file existed.
//
// It fires the workflow and gets out of the way. If the graph ends in a Device Gate, the push lands
// a second later and the root layout opens the approval screen on top of this one, which is the
// whole journey: tap → Face ID → the gate opens.
import { useEffect, useRef, useState } from "react";
import { ActivityIndicator, View } from "react-native";
import { router, useLocalSearchParams } from "expo-router";
import { runWorkflow } from "@/workflows";
import { Body, Card, Danger, Muted, Screen, Strong } from "@/styles/ui";
import { space, useTheme } from "@/styles/theme";

export default function RunById() {
  const { id } = useLocalSearchParams<{ id?: string }>();
  const [problem, setProblem] = useState<string | null>(null);
  const t = useTheme();
  // React can mount an effect twice; a workflow that opens a garage must fire ONCE per tap.
  const fired = useRef(false);

  useEffect(() => {
    if (!id || fired.current) return;
    fired.current = true;
    runWorkflow(id)
      .then(() => {
        // Straight back to the Run tab. Lingering on a spinner would put a dead screen between the
        // person and the approval that is about to arrive.
        router.replace("/(tabs)/run");
      })
      .catch((e) => setProblem((e as Error).message));
  }, [id]);

  return (
    <Screen>
      <View style={{ padding: space.xl, gap: space.md }}>
        {problem ? (
          <Card>
            <Strong>Couldn&apos;t start it</Strong>
            <Danger>{problem}</Danger>
            <Body>The workflow was not run. Open the app and try from the Run tab.</Body>
          </Card>
        ) : (
          <View style={{ flexDirection: "row", alignItems: "center", gap: space.md }}>
            <ActivityIndicator color={t.mutedForeground} />
            <Muted>Starting…</Muted>
          </View>
        )}
      </View>
    </Screen>
  );
}
