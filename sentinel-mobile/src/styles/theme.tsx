// Theme access for screens. The web flips on a `.dark` class; the phone follows the OS setting,
// which is the platform-native equivalent of the same monochrome ink theme (Clean White / Noir).
import { useColorScheme } from "react-native";
import { darkTheme, lightTheme, type Theme } from "./tokens";

export function useTheme(): Theme {
  return useColorScheme() === "dark" ? darkTheme : lightTheme;
}

export { radius, space, type as typeScale } from "./tokens";
export type { Theme };
