import type { ActivitySnapshot } from "../types";

export type ActivityTone = "idle" | "running" | "pending";

export function pendingTaskCount(activity: ActivitySnapshot): number {
  return activity.waitingOnApproval + activity.waitingOnUserInput;
}

export function activityTone(activity: ActivitySnapshot): ActivityTone {
  if (pendingTaskCount(activity) > 0) return "pending";
  if (activity.executing > 0) return "running";
  return "idle";
}
