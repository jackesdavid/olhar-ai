// The shared primitives every screen builds from, so a screen never reaches for a colour literal.
// Deliberately small — these mirror the web's card / button / muted-text roles, not a component kit.
import { ActivityIndicator, Pressable, StyleSheet, Text, TextInput, View, type TextInputProps, type ViewProps } from "react-native";
import { radius, space, setThemePref, typeScale, useTheme, useThemePref, type ThemePref } from "./theme";
import { Loader as MarkLoader, PulseDot } from "./Mark";

export { EmptyState, OlharMark, PulseDot } from "./Mark";

/** The loader: the dot-field breathing. Never a bare spinner — the mark IS the brand. */
export function Loader({ label }: { label?: string }) {
  const t = useTheme();
  return (
    <View style={{ alignItems: "center", gap: space.md, paddingVertical: space.xl }}>
      <MarkLoader />
      {label ? <Text style={[typeScale.body, { color: t.mutedForeground }]}>{label}</Text> : null}
    </View>
  );
}

export function Card({ style, ...rest }: ViewProps) {
  const t = useTheme();
  return (
    <View
      {...rest}
      style={[
        {
          backgroundColor: t.card,
          borderColor: t.border,
          // 1px, not hairline. On the web a card lifts off the page through hover, an ambient
          // shadow and a wide viewport; a phone has none of those, and at 3× a hairline over a
          // nine-value elevation step is a card you cannot see. Reported as exactly that.
          borderWidth: 1,
          borderRadius: radius.xl,
          padding: space.lg,
          gap: space.md,
          // Depth, subtle: the panel sits ABOVE the canvas rather than being tinted differently.
          shadowColor: "#000",
          shadowOpacity: 0.18,
          shadowRadius: 12,
          shadowOffset: { width: 0, height: 4 },
          elevation: 3,
        },
        style,
      ]}
    />
  );
}

/** A card you can press. Tactile feedback on every interactive surface — the web's `.press`. */
export function PressableCard({
  onPress,
  disabled,
  style,
  children,
}: {
  onPress: () => void;
  disabled?: boolean;
  style?: ViewProps["style"];
  children: React.ReactNode;
}) {
  return (
    <Pressable onPress={onPress} disabled={disabled}>
      {({ pressed }) => (
        <Card style={[pressed && { transform: [{ scale: 0.985 }], opacity: 0.92 }, style]}>
          {children}
        </Card>
      )}
    </Pressable>
  );
}

export function Title({ children }: { children: React.ReactNode }) {
  const t = useTheme();
  return <Text style={[typeScale.display, { color: t.foreground }]}>{children}</Text>;
}

export function Heading({ children }: { children: React.ReactNode }) {
  const t = useTheme();
  return <Text style={[typeScale.title, { color: t.foreground }]}>{children}</Text>;
}

export function Body({ children }: { children: React.ReactNode }) {
  const t = useTheme();
  return <Text style={[typeScale.body, { color: t.foreground }]}>{children}</Text>;
}

export function Muted({ children }: { children: React.ReactNode }) {
  const t = useTheme();
  return <Text style={[typeScale.body, { color: t.mutedForeground }]}>{children}</Text>;
}

/** Failure text — the one place the ink theme allows a hue, same as the web's `--destructive`. */
export function Danger({ children }: { children: React.ReactNode }) {
  const t = useTheme();
  return <Text style={[typeScale.body, { color: t.destructive }]}>{children}</Text>;
}

export function Strong({ children }: { children: React.ReactNode }) {
  const t = useTheme();
  return <Text style={{ fontWeight: "600", color: t.foreground }}>{children}</Text>;
}

/** Status + the signal ping. Liveness reads through MOTION, not a colour swatch — the ink theme
 *  has no brand green, and a pulsing dot says "right now" in a way a static one cannot. */
export function Status({ ok, children }: { ok: boolean; children: React.ReactNode }) {
  const t = useTheme();
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 2, marginLeft: -6 }}>
      <PulseDot live={ok} />
      <Text style={{ fontWeight: "600", color: ok ? t.foreground : t.destructive }}>{children}</Text>
    </View>
  );
}

export function Input(props: TextInputProps) {
  const t = useTheme();
  return (
    <TextInput
      placeholderTextColor={t.mutedForeground}
      {...props}
      style={[
        // Inputs SINK below their panel (elev-1), same as the web.
        { backgroundColor: t.inputBg, borderColor: t.input, borderWidth: StyleSheet.hairlineWidth, borderRadius: radius.lg, padding: space.md + 2, fontSize: 16, color: t.foreground },
        props.style,
      ]}
    />
  );
}

export function Button({ label, onPress, busy, disabled }: { label: string; onPress: () => void; busy?: boolean; disabled?: boolean }) {
  const t = useTheme();
  const off = busy || disabled;
  return (
    <Pressable
      onPress={onPress}
      disabled={off}
      style={({ pressed }) => [
        { backgroundColor: t.primary, borderRadius: radius.lg, padding: space.lg, alignItems: "center" },
        off && { opacity: 0.6 },
        // `.press` on the web — tactile scale feedback.
        pressed && { transform: [{ scale: 0.97 }] },
      ]}
    >
      {busy ? (
        <ActivityIndicator color={t.primaryForeground} />
      ) : (
        <Text style={{ color: t.primaryForeground, fontSize: 16, fontWeight: "600" }}>{label}</Text>
      )}
    </Pressable>
  );
}

/** Theme picker: System / Light / Dark, mirroring the web app's explicit choice. */
export function ThemePicker() {
  const t = useTheme();
  const pref = useThemePref();
  const options: { key: ThemePref; label: string }[] = [
    { key: "system", label: "System" },
    { key: "light", label: "Light" },
    { key: "dark", label: "Dark" },
  ];
  return (
    <View style={{ flexDirection: "row", gap: space.xs, backgroundColor: t.inputBg, borderRadius: radius.lg, padding: space.xs }}>
      {options.map((o) => {
        const on = pref === o.key;
        return (
          <Pressable
            key={o.key}
            onPress={() => setThemePref(o.key)}
            style={{ flex: 1, alignItems: "center", paddingVertical: space.sm, borderRadius: radius.md, backgroundColor: on ? t.card : "transparent" }}
          >
            <Text style={{ color: on ? t.foreground : t.mutedForeground, fontWeight: on ? "600" : "400" }}>{o.label}</Text>
          </Pressable>
        );
      })}
    </View>
  );
}

/** Page canvas — the base elevation stop, so cards read as panels lifted off it. */
export function Screen({ style, ...rest }: ViewProps) {
  const t = useTheme();
  return <View {...rest} style={[{ flex: 1, backgroundColor: t.background }, style]} />;
}
