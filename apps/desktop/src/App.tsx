import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { QuotaCard, QuotaOrb } from "./components/QuotaCard";
import { fetchSnapshots, getCompletionShutdownState, getPreferences, listenDesktopEvents, openSettings, setAlwaysOnTop, setCompletionShutdownArmed as updateCompletionShutdownArmed, setWidgetExpanded, startDragging, updatePreferences } from "./lib/bridge";
import { copy, normalizeLanguage } from "./lib/i18n";
import { mergeSnapshots } from "./lib/snapshots";
import type { ProviderSnapshot, WidgetPreferences } from "./types";

const DEFAULT_PREFS: WidgetPreferences = {
  locked: false,
  alwaysOnTop: true,
  stayExpanded: false,
  pinnedProvider: null,
  autoRotateSeconds: 12,
  language: "zh-CN",
  syncRole: "collector",
  serverUrl: "",
  sourceId: "windows-main",
  activityStatePath: "",
  shutdownScriptPath: "E:\\python\\shutdown.cmd",
};

export default function App() {
  const [snapshots, setSnapshots] = useState<ProviderSnapshot[]>([]);
  const [preferences, setPreferences] = useState(DEFAULT_PREFS);
  const [activeIndex, setActiveIndex] = useState(0);
  const [hovered, setHovered] = useState(false);
  const [compact, setCompact] = useState(true);
  const [consumingProviders, setConsumingProviders] = useState<Set<string>>(() => new Set());
  const [operationError, setOperationError] = useState<string | null>(null);
  const [completionShutdownArmed, setCompletionShutdownArmed] = useState(false);
  const failures = useRef(0);
  const previousPrimary = useRef(new Map<string, number>());
  const consumptionTimers = useRef(new Map<string, number>());
  const collapseTimer = useRef<number | null>(null);
  const hoverSequence = useRef(0);
  const language = normalizeLanguage(preferences.language);
  const t = copy[language];

  const refresh = useCallback(async (force = false) => {
    try {
      const values = await fetchSnapshots(force);
      const hasFailure = values.some((item) => item.status !== "ok");
      if (hasFailure) failures.current += 1;
      else failures.current = 0;
      for (const item of values) {
        const nextPrimary = item.shortWindow?.remainingPercent;
        const previous = previousPrimary.current.get(item.provider);
        if (nextPrimary !== undefined && previous !== undefined && nextPrimary < previous) {
          setConsumingProviders((current) => new Set(current).add(item.provider));
          const oldTimer = consumptionTimers.current.get(item.provider);
          if (oldTimer !== undefined) window.clearTimeout(oldTimer);
          const timer = window.setTimeout(() => {
            setConsumingProviders((current) => { const next = new Set(current); next.delete(item.provider); return next; });
            consumptionTimers.current.delete(item.provider);
          }, 5 * 60_000);
          consumptionTimers.current.set(item.provider, timer);
        }
        if (nextPrimary !== undefined) previousPrimary.current.set(item.provider, nextPrimary);
      }
      setSnapshots((current) => mergeSnapshots(current, values));
    } catch {
      failures.current += 1;
      setSnapshots((current) => current.length > 0
        ? current.map((item) => ({ ...item, status: "stale", message: "Refresh failed. Please try again later." }))
        : [{
          provider: "codex",
          displayName: "CODEX",
          plan: null,
          shortWindow: null,
          weeklyWindow: null,
          resetCredits: null,
          resetCreditExpiresAt: [],
          updatedAt: new Date().toISOString(),
          status: "unavailable",
          message: "Quota is temporarily unavailable. It will retry automatically.",
          nextResetAt: null,
          nextResetWindow: null,
          activity: { executing: 0, waitingOnApproval: 0, waitingOnUserInput: 0, source: "unavailable", observedAt: new Date().toISOString(), stale: true },
          sync: { role: preferences.syncRole, state: "offline", sourceId: preferences.sourceId, collectedAt: null, receivedAt: null, message: "Refresh failed." },
        }]);
    }
  // 同步端点或 Hooks 状态文件变更后，重新创建刷新回调会触发现有的强制刷新 effect，
  // 使设置页保存立即生效，而不是等待下一次定时刷新。
  }, [preferences.activityStatePath, preferences.serverUrl, preferences.sourceId, preferences.syncRole]);

  useEffect(() => {
    void refresh(true);
    void getPreferences().then((value) => setPreferences({ ...DEFAULT_PREFS, ...value, language: normalizeLanguage(value.language) })).catch(() => setOperationError("Unable to read settings. Defaults are in use."));
    void getCompletionShutdownState().then((value) => setCompletionShutdownArmed(value.armed)).catch(() => setOperationError("Unable to read completion shutdown state."));
    return () => {
      for (const timer of consumptionTimers.current.values()) window.clearTimeout(timer);
      consumptionTimers.current.clear();
      if (collapseTimer.current !== null) window.clearTimeout(collapseTimer.current);
    };
  }, [refresh]);

  useEffect(() => {
    let cancelled = false;
    let cleanup: () => void = () => {};
    void listenDesktopEvents({
      onPreferences: (value) => setPreferences({ ...DEFAULT_PREFS, ...value, language: normalizeLanguage(value.language) }),
      onRefresh: () => void refresh(true),
      onCompletionShutdown: (value) => setCompletionShutdownArmed(value.armed),
      onCompletionShutdownNotice: (message) => setOperationError(message),
    }).then((value) => {
      if (cancelled) value(); else cleanup = value;
    }).catch(() => setOperationError("Desktop event listener failed to start."));
    return () => { cancelled = true; cleanup(); };
  }, [refresh]);

  const refreshMs = useMemo(() => failures.current === 0
    ? 60_000
    : Math.min(15 * 60_000, 30_000 * 2 ** (failures.current - 1)), [snapshots]);

  useEffect(() => {
    const id = window.setInterval(() => void refresh(), refreshMs);
    return () => window.clearInterval(id);
  }, [refresh, refreshMs]);

  useEffect(() => {
    const refreshWhenActive = () => { if (document.visibilityState === "visible") void refresh(); };
    window.addEventListener("focus", refreshWhenActive);
    document.addEventListener("visibilitychange", refreshWhenActive);
    return () => {
      window.removeEventListener("focus", refreshWhenActive);
      document.removeEventListener("visibilitychange", refreshWhenActive);
    };
  }, [refresh]);

  useEffect(() => {
    if (hovered || preferences.pinnedProvider || snapshots.length < 2) return;
    const id = window.setInterval(() => setActiveIndex((value) => (value + 1) % snapshots.length), preferences.autoRotateSeconds * 1000);
    return () => window.clearInterval(id);
  }, [hovered, preferences.autoRotateSeconds, preferences.pinnedProvider, snapshots.length]);

  const current = preferences.pinnedProvider
    ? snapshots.find((item) => item.provider === preferences.pinnedProvider) ?? snapshots[0]
    : snapshots[activeIndex % Math.max(1, snapshots.length)];

  const savePreferences = useCallback((next: WidgetPreferences) => {
    const previous = preferences;
    setPreferences(next);
    setOperationError(null);
    void updatePreferences(next).catch(() => { setPreferences(previous); setOperationError("Settings could not be saved. Previous state restored."); });
  }, [preferences]);

  const toggleCompletionShutdown = useCallback(() => {
    setOperationError(null);
    void updateCompletionShutdownArmed(!completionShutdownArmed)
      .then((value) => {
        setCompletionShutdownArmed(value.armed);
        if (value.armed) void refresh(true);
      })
      .catch((reason: unknown) => setOperationError(reason instanceof Error ? reason.message : "Unable to update completion shutdown."));
  }, [completionShutdownArmed, refresh]);

  const handleOpenSettings = useCallback(() => {
    setOperationError(null);
    void openSettings().catch((reason: unknown) => setOperationError(reason instanceof Error ? reason.message : "Unable to open settings."));
  }, []);

  const handleHover = useCallback((value: boolean) => {
    if (collapseTimer.current !== null) {
      window.clearTimeout(collapseTimer.current);
      collapseTimer.current = null;
    }
    setHovered(value);
    if (!value && preferences.stayExpanded) return;
    if (value) void refresh();
    if (value) {
      const sequence = ++hoverSequence.current;
      void setWidgetExpanded(true)
        .then(() => { if (hoverSequence.current === sequence) setCompact(false); })
        .catch(() => {
          setCompact(false);
          setOperationError("Widget expand failed.");
        });
      return;
    }
    const sequence = ++hoverSequence.current;
    collapseTimer.current = window.setTimeout(() => {
      if (hoverSequence.current !== sequence) return;
      setCompact(true);
      void setWidgetExpanded(false).catch(() => setOperationError("Widget collapse failed."));
    }, 180);
  }, [preferences.stayExpanded, refresh]);

  useEffect(() => {
    if (!preferences.stayExpanded) return;
    if (collapseTimer.current !== null) window.clearTimeout(collapseTimer.current);
    setCompact(false);
    void setWidgetExpanded(true).catch(() => setOperationError("Widget expand failed."));
  }, [preferences.stayExpanded]);

  if (!current) return <div className="loading-card" aria-label={t.loadingQuota}><span /><span /><span /></div>;

  if (compact) {
    return <QuotaOrb snapshot={current} language={language} onDrag={() => startDragging()} onHover={handleHover} />;
  }

  return (
    <QuotaCard
      snapshot={current}
      preferences={preferences}
      providerCount={snapshots.length}
      onPrevious={() => setActiveIndex((value) => (value - 1 + snapshots.length) % snapshots.length)}
      onNext={() => setActiveIndex((value) => (value + 1) % snapshots.length)}
      onTogglePin={() => savePreferences({ ...preferences, pinnedProvider: preferences.pinnedProvider ? null : current.provider })}
      onToggleStayExpanded={() => savePreferences({ ...preferences, stayExpanded: !preferences.stayExpanded })}
      onLock={() => { setOperationError(null); void setAlwaysOnTop(!preferences.alwaysOnTop).then((value) => setPreferences({ ...DEFAULT_PREFS, ...value, language: normalizeLanguage(value.language) })).catch(() => setOperationError("Always-on-top toggle failed.")); }}
      onDrag={() => startDragging()}
      onHover={handleHover}
      onRefresh={() => refresh(true)}
      completionShutdownArmed={completionShutdownArmed}
      onToggleCompletionShutdown={toggleCompletionShutdown}
      onOpenSettings={handleOpenSettings}
      isConsuming={consumingProviders.has(current.provider)}
      notice={operationError}
    />
  );
}
