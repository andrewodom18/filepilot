import { getVersion } from "@tauri-apps/api/app";

export async function loadAppVersion(
  versionReader: () => Promise<string> = getVersion,
): Promise<string> {
  try {
    const version = (await versionReader()).trim();
    return version || "Unknown";
  } catch {
    return "Unknown";
  }
}
