import { type KeyboardEvent, useEffect, useState } from "react";
import {
  getAppStatus,
  getCuratedModels,
  getSettings,
  listenEditorLoadDraft,
  listenMainNavigate,
  takeEditorDraft,
  type AppStatusDto,
  type MainTab,
  type ModelOptionDto,
  type SettingsDto,
} from "../lib/ipc";
import { MESSAGES } from "../lib/messages";
import { HistoryTab } from "./HistoryTab";
import { SettingsTab } from "./SettingsTab";
import { WelcomeView } from "./WelcomeView";
import { WriteTab } from "./WriteTab";

export function MainApp() {
  const [status, setStatus] = useState<AppStatusDto | null>(null);
  const [settings, setSettings] = useState<SettingsDto | null>(null);
  const [models, setModels] = useState<ModelOptionDto[]>([]);
  const [activeTab, setActiveTab] = useState<MainTab>("write");
  const [editorText, setEditorText] = useState("");
  const [editorSourceApp, setEditorSourceApp] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    Promise.all([getAppStatus(), getSettings(), getCuratedModels(), takeEditorDraft()])
      .then(([loadedStatus, loadedSettings, loadedModels, draft]) => {
        if (!active) return;
        setStatus(loadedStatus);
        setSettings(loadedSettings);
        setModels(loadedModels);
        if (loadedStatus.hotkeyConflict) {
          setActiveTab("settings");
        }
        if (draft) {
          setEditorText(draft.text);
          setEditorSourceApp(draft.sourceApp);
          setActiveTab("write");
        }
      })
      .catch(() => {});

    let unlistenNav: (() => void) | null = null;
    let unlistenDraft: (() => void) | null = null;

    listenMainNavigate((tab) => {
      setActiveTab(tab);
    })
      .then((fn) => {
        unlistenNav = fn;
      })
      .catch(() => {});

    listenEditorLoadDraft((draft) => {
      setEditorText(draft.text);
      setEditorSourceApp(draft.sourceApp);
      setActiveTab("write");
    })
      .then((fn) => {
        unlistenDraft = fn;
      })
      .catch(() => {});

    return () => {
      active = false;
      if (unlistenNav) unlistenNav();
      if (unlistenDraft) unlistenDraft();
    };
  }, []);

  if (!status || !settings) {
    return <main className="h-full bg-canvas p-6 text-sm text-ink-muted" />;
  }

  if (!status.welcomeCompleted && !status.hotkeyConflict) {
    return (
      <WelcomeView
        status={status}
        onComplete={({ hasApiKey, autostartEnabled }) => {
          setStatus({ ...status, hasApiKey, autostartEnabled, welcomeCompleted: true });
          setSettings({ ...settings, welcomeCompleted: true });
        }}
      />
    );
  }

  const tabs: { id: MainTab; label: string }[] = [
    { id: "write", label: MESSAGES.tabs.write },
    { id: "history", label: MESSAGES.tabs.history },
    { id: "settings", label: MESSAGES.tabs.settings },
  ];

  function handleTabKeyDown(e: KeyboardEvent<HTMLButtonElement>, index: number) {
    let nextIndex: number | null = null;
    if (e.key === "ArrowRight") nextIndex = (index + 1) % tabs.length;
    else if (e.key === "ArrowLeft") nextIndex = (index - 1 + tabs.length) % tabs.length;
    else if (e.key === "Home") nextIndex = 0;
    else if (e.key === "End") nextIndex = tabs.length - 1;
    if (nextIndex === null) return;
    e.preventDefault();
    const targetTab = tabs[nextIndex];
    if (!targetTab) return;
    setActiveTab(targetTab.id);
    document.getElementById(`tab-${targetTab.id}`)?.focus();
  }

  return (
    <main className="flex h-full flex-col gap-4 bg-canvas p-5 text-ink">
      <header className="flex items-center justify-between gap-4 border-b border-line pb-3">
        <div className="flex items-center gap-4">
          <h1 className="text-base font-semibold tracking-tight text-ink">{MESSAGES.app.name}</h1>
          <nav
            aria-label={MESSAGES.a11y.mainSections}
            role="tablist"
            className="flex items-center gap-1"
          >
            {tabs.map((tab, idx) => {
              const selected = activeTab === tab.id;
              return (
                <button
                  key={tab.id}
                  id={`tab-${tab.id}`}
                  type="button"
                  role="tab"
                  aria-selected={selected}
                  aria-controls={`panel-${tab.id}`}
                  tabIndex={selected ? 0 : -1}
                  onClick={() => setActiveTab(tab.id)}
                  onKeyDown={(e) => handleTabKeyDown(e, idx)}
                  className={`rounded-md px-3 py-1 text-xs font-medium transition outline-none focus-visible:ring-2 focus-visible:ring-accent ${
                    selected
                      ? "bg-accent text-accent-contrast"
                      : "text-ink-secondary hover:bg-hover hover:text-ink"
                  }`}
                >
                  {tab.label}
                </button>
              );
            })}
          </nav>
        </div>

        <kbd className="rounded-sm border border-line-strong bg-elevated px-2 py-0.5 font-mono text-xs text-ink-secondary">
          {settings.hotkey}
        </kbd>
      </header>

      <div
        id={`panel-${activeTab}`}
        role="tabpanel"
        aria-labelledby={`tab-${activeTab}`}
        className="flex min-h-0 flex-1 flex-col"
      >
        {activeTab === "write" && (
          <WriteTab
            text={editorText}
            sourceApp={editorSourceApp}
            recentCustom={settings.recentCustomInstructions}
            onTextChange={setEditorText}
            onRecentCustomChange={(next) =>
              setSettings((prev) => (prev ? { ...prev, recentCustomInstructions: next } : prev))
            }
            onOpenSettings={() => setActiveTab("settings")}
          />
        )}

        {activeTab === "history" && (
          <HistoryTab
            historyEnabled={settings.historyEnabled}
            historyRecovered={status.historyRecovered}
            onHistoryCleared={() =>
              setStatus((prev) => (prev ? { ...prev, historyRecovered: false } : prev))
            }
          />
        )}

        {activeTab === "settings" && (
          <SettingsTab
            status={status}
            settings={settings}
            models={models}
            onStatusChange={setStatus}
            onSettingsChange={setSettings}
          />
        )}
      </div>
    </main>
  );
}
