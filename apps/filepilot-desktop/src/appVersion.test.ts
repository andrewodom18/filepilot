import { describe, expect, it } from "vitest";
import { loadAppVersion } from "./appVersion";

describe("loadAppVersion", () => {
  it("uses the version embedded in the running Tauri bundle", async () => {
    await expect(loadAppVersion(async () => "2.1.1")).resolves.toBe("2.1.1");
  });

  it("does not show a stale hard-coded version when metadata is unavailable", async () => {
    await expect(loadAppVersion(async () => { throw new Error("unavailable"); })).resolves.toBe("Unknown");
  });
});
