// Where the app reaches the backend. Mirrors `sentinel-app/src/lib/config.ts`, but a phone has no
// `window.location` to infer prod from — so the default IS production and a dev override comes from
// an env var read at bundle time (`EXPO_PUBLIC_*` is inlined by Expo).
//
// Pointing a simulator at a laptop: `EXPO_PUBLIC_CONTROL_URL=http://192.168.x.x:8790 npx expo start`.
// Note `localhost` does NOT work from a physical device — use the LAN IP.
const trim = (u: string) => u.replace(/\/+$/, "");

export const CONTROL_BASE = trim(process.env.EXPO_PUBLIC_CONTROL_URL || "https://control.olharai.com");

export const GATEWAY_BASE = trim(process.env.EXPO_PUBLIC_GATEWAY_URL || "https://gateway.olharai.com");
