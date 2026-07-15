import { describe, expect, it } from "vitest";
import type { ActivitySnapshot } from "../types";
import { activityTone, pendingTaskCount } from "./activityPresentation";

function activity(executing: number, approval: number, input: number): ActivitySnapshot {
  return {
    executing,
    waitingOnApproval: approval,
    waitingOnUserInput: input,
    source: "hooks",
    observedAt: "2026-07-15T00:00:00Z",
    stale: false,
  };
}

describe("activity presentation", () => {
  it("merges approval and input into pending", () => {
    expect(pendingTaskCount(activity(0, 2, 3))).toBe(5);
  });

  it.each([
    [activity(0, 0, 0), "idle"],
    [activity(2, 0, 0), "running"],
    [activity(2, 1, 0), "pending"],
    [activity(0, 0, 1), "pending"],
  ] as const)("maps activity to the expected tone", (value, expected) => {
    expect(activityTone(value)).toBe(expected);
  });
});
