import { Check, FloppyDisk, Power, X } from "@phosphor-icons/react";
import { type FormEvent, useEffect, useMemo, useState } from "react";
import {
  getCompletionShutdownState,
  getPreferences,
  hideCurrentWindow,
  listenDesktopEvents,
  setCompletionShutdownArmed,
  setCollectorWriteSecret,
  updatePreferences,
} from "../lib/bridge";
import { composeServerUrl, splitServerUrl } from "../lib/serverEndpoint";
import { copy, normalizeLanguage } from "../lib/i18n";
import type { CompletionShutdownState, WidgetPreferences } from "../types";

interface SettingsForm {
  syncRole: WidgetPreferences["syncRole"];
  serverHost: string;
  serverPort: string;
  sourceId: string;
  activityStatePath: string;
  shutdownScriptPath: string;
  writeSecret: string;
}

function formFromPreferences(preferences: WidgetPreferences): SettingsForm {
  const endpoint = splitServerUrl(preferences.serverUrl);
  return {
    syncRole: preferences.syncRole,
    serverHost: endpoint.host,
    serverPort: endpoint.port,
    sourceId: preferences.sourceId,
    activityStatePath: preferences.activityStatePath,
    shutdownScriptPath: preferences.shutdownScriptPath,
    // 写入密钥永不从 preferences 返回给前端；这个字段仅承载当前输入。
    writeSecret: "",
  };
}

function messageFrom(error: unknown): string {
  return error instanceof Error && error.message ? error.message : "设置操作失败，请稍后重试。";
}

export function SettingsPage() {
  const [preferences, setPreferences] = useState<WidgetPreferences | null>(null);
  const [form, setForm] = useState<SettingsForm | null>(null);
  const [shutdownState, setShutdownState] = useState<CompletionShutdownState>({ armed: false });
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);
  const language = normalizeLanguage(preferences?.language);
  const t = copy[language];

  useEffect(() => {
    let cancelled = false;
    let cleanup: () => void = () => {};
    void Promise.all([getPreferences(), getCompletionShutdownState()])
      .then(([nextPreferences, nextShutdownState]) => {
        if (cancelled) return;
        setPreferences(nextPreferences);
        setForm(formFromPreferences(nextPreferences));
        setShutdownState(nextShutdownState);
      })
      .catch((reason) => { if (!cancelled) setError(messageFrom(reason)); });
    void listenDesktopEvents({
      onPreferences: (nextPreferences) => {
        if (cancelled) return;
        setPreferences(nextPreferences);
        setForm(formFromPreferences(nextPreferences));
      },
      onRefresh: () => undefined,
      onCompletionShutdown: (value) => { if (!cancelled) setShutdownState(value); },
      onCompletionShutdownNotice: (message) => { if (!cancelled) setError(message); },
    }).then((unlisten) => {
      if (cancelled) unlisten(); else cleanup = unlisten;
    }).catch((reason) => { if (!cancelled) setError(messageFrom(reason)); });
    return () => { cancelled = true; cleanup(); };
  }, []);

  const isCollector = form?.syncRole === "collector";
  const canSave = Boolean(preferences && form) && !saving;
  const statusLabel = useMemo(() => shutdownState.armed ? t.completionShutdownArmed : t.completionShutdownDisarmed, [shutdownState.armed, t]);

  const updateForm = <Key extends keyof SettingsForm>(key: Key, value: SettingsForm[Key]) => {
    setForm((current) => current ? { ...current, [key]: value } : current);
    setSaved(false);
  };

  const handleSave = async (event: FormEvent) => {
    event.preventDefault();
    if (!preferences || !form) return;
    setError(null);
    setSaved(false);
    setSaving(true);
    try {
      const next: WidgetPreferences = {
        ...preferences,
        syncRole: form.syncRole,
        serverUrl: composeServerUrl(form.serverHost, form.serverPort),
        sourceId: form.sourceId.trim(),
        activityStatePath: form.activityStatePath.trim(),
        shutdownScriptPath: form.shutdownScriptPath.trim(),
      };
      const writeSecret = form.writeSecret;
      await updatePreferences(next);
      if (form.syncRole === "collector" && writeSecret.length > 0) {
        await setCollectorWriteSecret(writeSecret);
      }
      setPreferences(next);
      setForm(formFromPreferences(next));
      setSaved(true);
    } catch (reason) {
      setError(messageFrom(reason));
    } finally {
      setSaving(false);
    }
  };

  const toggleCompletionShutdown = async () => {
    if (!isCollector) return;
    setError(null);
    try {
      setShutdownState(await setCompletionShutdownArmed(!shutdownState.armed));
    } catch (reason) {
      setError(messageFrom(reason));
    }
  };

  if (!preferences || !form) {
    return <main className="settings-shell settings-shell--loading" aria-live="polite"><span /><span /><span /></main>;
  }

  return (
    <main className="settings-shell">
      <header className="settings-header">
        <div>
          <p>CODEX QUOTA SYNC</p>
          <h1>{t.settings}</h1>
          <span>{t.settingsDescription}</span>
        </div>
        <button type="button" className="settings-close" onClick={() => { void hideCurrentWindow(); }} aria-label={t.closeSettings} title={t.closeSettings}><X /></button>
      </header>

      <form className="settings-form" onSubmit={handleSave}>
        <section className="settings-section">
          <h2>{t.syncRole}</h2>
          <label className="settings-field">
            <span>{t.syncRole}</span>
            <select value={form.syncRole} onChange={(event) => updateForm("syncRole", event.target.value as WidgetPreferences["syncRole"])}>
              <option value="collector">{t.syncRoleCollector}</option>
              <option value="viewer">{t.syncRoleViewer}</option>
            </select>
          </label>
          <div className="settings-grid">
            <label className="settings-field settings-field--wide">
              <span>{t.serverAddress}</span>
              <input value={form.serverHost} onChange={(event) => updateForm("serverHost", event.target.value)} placeholder="http://10.10.10.254" spellCheck={false} />
              <small>{t.serverAddressHint}</small>
            </label>
            <label className="settings-field">
              <span>{t.serverPort}</span>
              <input value={form.serverPort} onChange={(event) => updateForm("serverPort", event.target.value)} inputMode="numeric" placeholder="18080" />
            </label>
          </div>
          {isCollector ? (
            <label className="settings-field">
              <span>{t.writeSecret}</span>
              <input type="password" value={form.writeSecret} onChange={(event) => updateForm("writeSecret", event.target.value)} autoComplete="off" placeholder={t.writeSecretPlaceholder} spellCheck={false} />
              <small>{t.writeSecretHint}</small>
            </label>
          ) : null}
          <label className="settings-field">
            <span>{t.sourceId}</span>
            <input value={form.sourceId} onChange={(event) => updateForm("sourceId", event.target.value)} placeholder="windows-main" spellCheck={false} />
            <small>{t.sourceIdHint}</small>
          </label>
          <label className="settings-field">
            <span>{t.activityStatePath}</span>
            <input value={form.activityStatePath} onChange={(event) => updateForm("activityStatePath", event.target.value)} placeholder={t.activityStatePathHint} spellCheck={false} />
          </label>
        </section>

        <section className="settings-section settings-section--shutdown">
          <div className="settings-section-heading">
            <div>
              <h2>{t.shutdownAfterCompletion}</h2>
              <p>{isCollector ? t.shutdownAfterCompletionHint : t.shutdownViewerHint}</p>
            </div>
            <button type="button" className={`shutdown-switch${shutdownState.armed ? " is-armed" : ""}`} disabled={!isCollector} onClick={toggleCompletionShutdown} aria-pressed={shutdownState.armed} aria-label={statusLabel} title={statusLabel}>
              <Power weight={shutdownState.armed ? "fill" : "regular"} />
              <span>{shutdownState.armed ? "ON" : "OFF"}</span>
            </button>
          </div>
          <label className="settings-field">
            <span>{t.shutdownScript}</span>
            <input value={form.shutdownScriptPath} onChange={(event) => updateForm("shutdownScriptPath", event.target.value)} placeholder="E:\\python\\shutdown.cmd" spellCheck={false} />
            <small>{t.shutdownScriptHint}</small>
          </label>
        </section>

        {error ? <p className="settings-message settings-message--error" role="alert">{error}</p> : null}
        {saved ? <p className="settings-message settings-message--success" role="status"><Check weight="bold" />{t.settingsSaved}</p> : null}
        <footer className="settings-footer">
          <button type="button" className="settings-secondary" onClick={() => { void hideCurrentWindow(); }}>{t.closeSettings}</button>
          <button type="submit" className="settings-primary" disabled={!canSave}><FloppyDisk weight="bold" />{saving ? t.savingSettings : t.saveSettings}</button>
        </footer>
      </form>
    </main>
  );
}
