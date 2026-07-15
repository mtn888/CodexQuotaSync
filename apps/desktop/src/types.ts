export type ProviderId = "codex" | "claude";
export type SnapshotStatus = "ok" | "stale" | "loading" | "unavailable" | "signed_out";
export type Language = "zh-CN" | "en";

export interface UsageWindow {
  remainingPercent: number;
  resetsAt: string | null;
  windowSeconds: number | null;
}

export interface ActivitySnapshot {
  executing: number;
  waitingOnApproval: number;
  waitingOnUserInput: number;
  source: "hooks" | "unavailable";
  observedAt: string;
  stale: boolean;
}

export type SyncRole = "collector" | "viewer";
export type SyncState = "local" | "synced" | "stale" | "offline" | "configuration";

export interface SyncView {
  role: SyncRole;
  state: SyncState;
  sourceId: string;
  collectedAt: string | null;
  receivedAt: string | null;
  message: string | null;
}

export interface ProviderSnapshot {
  provider: ProviderId;
  displayName: string;
  plan: string | null;
  shortWindow: UsageWindow | null;
  weeklyWindow: UsageWindow | null;
  resetCredits: number | null;
  resetCreditExpiresAt?: string[];
  updatedAt: string;
  status: SnapshotStatus;
  message: string | null;
  nextResetAt: string | null;
  nextResetWindow: "5h" | "weekly" | null;
  activity: ActivitySnapshot;
  sync: SyncView;
}

export interface WidgetPreferences {
  locked: boolean;
  alwaysOnTop: boolean;
  stayExpanded: boolean;
  pinnedProvider: ProviderId | null;
  autoRotateSeconds: number;
  language: Language;
  syncRole: SyncRole;
  serverUrl: string;
  sourceId: string;
  activityStatePath: string;
}
