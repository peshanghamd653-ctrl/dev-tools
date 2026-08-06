import { create } from "zustand";
import { persist } from "zustand/middleware";

import {
  DEFAULT_THEME,
  isThemePreference,
  THEME_STORAGE_KEY,
  type ThemePreference,
} from "@/shared/theme/themes";

interface ThemeState {
  preference: ThemePreference;
  setPreference: (preference: ThemePreference) => void;
}

/**
 * Persisted under `devos-theme`. The boot script in `index.html` reads this
 * exact key/shape synchronously before first paint, so the storage name, the
 * `state.preference` path and the default must not drift — see
 * `src/shared/theme/themes.test.ts`.
 */
export const useThemeStore = create<ThemeState>()(
  persist(
    (set) => ({
      preference: DEFAULT_THEME,
      setPreference: (preference) => set({ preference }),
    }),
    {
      name: THEME_STORAGE_KEY,
      partialize: (s) => ({ preference: s.preference }),
      // A value written by a newer build (or hand-edited) must not leave the
      // app unstyled; fall back rather than trusting whatever is on disk.
      merge: (persisted, current) => {
        const stored = (persisted as Partial<ThemeState> | undefined)
          ?.preference;
        return {
          ...current,
          preference: isThemePreference(stored) ? stored : DEFAULT_THEME,
        };
      },
    },
  ),
);
