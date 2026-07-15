import { describe, expect, it } from "vitest";
import type { ProviderSnapshot } from "../types";
import { mergeSnapshots } from "./snapshots";

const success: ProviderSnapshot = {
  provider: "codex",
  displayName: "CODEX",
  plan: "PRO",
  shortWindow: { remainingPercent: 74, resetsAt: "2026-07-07T02:00:00Z", windowSeconds: 18_000 },
  weeklyWindow: { remainingPercent: 42, resetsAt: "2026-07-10T00:00:00Z", windowSeconds: 604_800 },
  resetCredits: 1,
  updatedAt: "2026-07-07T00:00:00Z",
  status: "ok",
  message: null,
  nextResetAt: "2026-07-07T02:00:00Z",
  nextResetWindow: "5h",
  activity: { executing: 0, waitingOnApproval: 0, waitingOnUserInput: 0, source: "hooks", observedAt: "2026-07-07T00:00:00Z", stale: false },
  sync: { role: "collector", state: "synced", sourceId: "windows-main", collectedAt: "2026-07-07T00:00:00Z", receivedAt: null, message: null },
};

describe("snapshot failure handling", () => {
  it("retains the last successful values during a transient failure", () => {
    const failure: ProviderSnapshot = { ...success, shortWindow: null, weeklyWindow: null, status: "unavailable", message: "Network unavailable", updatedAt: "2026-07-07T01:00:00Z" };
    expect(mergeSnapshots([success], [failure])[0]).toEqual({ ...success, activity: failure.activity, sync: failure.sync, status: "stale", message: "Network unavailable" });
  });

  it("shows a failure when no successful snapshot exists", () => {
    const signedOut: ProviderSnapshot = { ...success, shortWindow: null, weeklyWindow: null, status: "signed_out", message: "Please sign in" };
    expect(mergeSnapshots([], [signedOut])[0].status).toBe("signed_out");
  });

  it("does not hide an expired login behind stale quota data", () => {
    const signedOut: ProviderSnapshot = { ...success, shortWindow: null, weeklyWindow: null, status: "signed_out", message: "Please sign in" };
    expect(mergeSnapshots([success], [signedOut])[0].status).toBe("signed_out");
  });

  it("replaces stale data after recovery", () => {
    expect(mergeSnapshots([{ ...success, status: "stale" }], [{ ...success, shortWindow: { ...success.shortWindow!, remainingPercent: 88 } }])[0].shortWindow?.remainingPercent).toBe(88);
  });

  it("keeps live activity and connection state while retaining stale quota", () => {
    const failure: ProviderSnapshot = {
      ...success,
      shortWindow: null,
      weeklyWindow: null,
      status: "unavailable",
      message: "offline",
      activity: { ...success.activity, executing: 3, waitingOnApproval: 1 },
      sync: { ...success.sync, state: "offline" },
    };
    const merged = mergeSnapshots([success], [failure])[0];
    expect(merged.shortWindow?.remainingPercent).toBe(74);
    expect(merged.activity.executing).toBe(3);
    expect(merged.activity.waitingOnApproval).toBe(1);
    expect(merged.sync.state).toBe("offline");
  });
});
