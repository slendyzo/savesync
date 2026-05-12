import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type AvailableUpdate = {
  version: string;
  body: string | null;
  install: () => Promise<void>;
};

/**
 * Check GitHub Releases for a newer published build. Returns null when
 * we're already on the latest version, or when the updater isn't
 * available (dev builds before CI has produced a signed manifest).
 * Errors are caught and surfaced as null so the UI never crashes on
 * a transient network issue.
 */
export async function checkForUpdate(): Promise<AvailableUpdate | null> {
  try {
    const update = await check();
    if (!update) return null;
    return {
      version: update.version,
      body: update.body ?? null,
      install: async () => {
        await update.downloadAndInstall();
        await relaunch();
      },
    };
  } catch (e) {
    console.warn("update check failed:", e);
    return null;
  }
}
