// Push registration (card #139, Phase 1).
//
// The device registers its Expo push token with control after sign-in. Per the operator's decision,
// **a device is its own Recipient** — the `label` we send here IS the name that appears in the
// workflow editor's recipient picker, so a graph can address this exact phone. Keep it human
// ("Jackes — iPhone"), because that is what someone picks from a dropdown months later.
import * as Device from "expo-device";
import * as Notifications from "expo-notifications";
import Constants from "expo-constants";
import { Platform } from "react-native";
import { api } from "./session";

/** Foreground behaviour: a security alert should interrupt, so show it even with the app open.
 * `shouldShowBanner`/`shouldShowList` replaced the old single `shouldShowAlert` flag. */
Notifications.setNotificationHandler({
  handleNotification: async () => ({
    shouldShowBanner: true,
    shouldShowList: true,
    shouldPlaySound: true,
    shouldSetBadge: false,
  }),
});

export type RegisterResult = { ok: true; label: string } | { ok: false; reason: string };

/** Ask the OS for permission, mint an Expo push token, and register it with control. */
export async function registerThisDevice(): Promise<RegisterResult> {
  // Push tokens are issued to real hardware only — a simulator will fail with an opaque error,
  // so say why up front instead of surfacing that.
  if (!Device.isDevice) {
    return { ok: false, reason: "Push needs a physical device — a simulator cannot receive notifications." };
  }

  if (Platform.OS === "android") {
    // Android 8+ drops notifications that have no channel.
    await Notifications.setNotificationChannelAsync("default", {
      name: "Alerts",
      importance: Notifications.AndroidImportance.HIGH,
    });
  }

  const existing = await Notifications.getPermissionsAsync();
  let status = existing.status;
  if (status !== "granted") {
    status = (await Notifications.requestPermissionsAsync()).status;
  }
  if (status !== "granted") {
    return { ok: false, reason: "Notification permission denied. Enable it in system settings." };
  }

  // SDK 49+ requires the EAS project id to mint a token; without it the call throws an error that
  // reads like a network fault. Fail with the actual cause.
  const projectId =
    Constants.expoConfig?.extra?.eas?.projectId ?? (Constants as any).easConfig?.projectId;
  if (!projectId || projectId === "REPLACE_WITH_EAS_PROJECT_ID") {
    return { ok: false, reason: "No EAS project id — run `eas init` and put the id in app.json." };
  }

  let deviceToken: string;
  try {
    deviceToken = (await Notifications.getExpoPushTokenAsync({ projectId })).data;
  } catch (e) {
    return { ok: false, reason: `Could not get a push token: ${String(e)}` };
  }

  const label = Device.deviceName?.trim() || `${Device.modelName ?? "Phone"} (${Platform.OS})`;
  try {
    await api("/push/devices", {
      method: "POST",
      body: JSON.stringify({ device_token: deviceToken, platform: Platform.OS, label }),
    });
  } catch (e) {
    return { ok: false, reason: `Registration failed: ${(e as Error).message}` };
  }
  return { ok: true, label };
}

/** Fire the server-side test push at this phone's Recipient label — proves delivery end to end. */
export async function sendTestPush(label: string): Promise<number> {
  const r = await api<{ sent: number }>("/push/devices/test", {
    method: "POST",
    body: JSON.stringify({ label }),
  });
  return r.sent;
}
