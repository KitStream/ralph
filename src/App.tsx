import { useState, useMemo, useEffect } from "react";
import { useSessions } from "./hooks/useSessions";
import { useAppUpdate } from "./hooks/useAppUpdate";
import { SidebarLayout } from "./components/layouts/SidebarLayout";
import { TabsLayout } from "./components/layouts/TabsLayout";
import { SplitLayout } from "./components/layouts/SplitLayout";
import { NewSessionDialog } from "./components/NewSessionDialog";
import { SettingsDialog } from "./components/SettingsDialog";
import { RecoveryDialog } from "./components/RecoveryDialog";
import { UpdateBanner } from "./components/UpdateBanner";
import type { SessionState, RecoveryRequest } from "./lib/types";

function App() {
  const { state, setActiveSession } = useSessions();
  const [showNewSession, setShowNewSession] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const update = useAppUpdate();

  useEffect(() => {
    document.documentElement.setAttribute(
      "data-theme",
      state.settings.theme.toLowerCase()
    );
  }, [state.settings.theme]);

  useEffect(() => {
    update.checkForUpdate().catch(() => {});
    // Only on mount — manual re-checks go through Settings.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const sessions: SessionState[] = useMemo(
    () => Array.from(state.sessions.values()),
    [state.sessions]
  );

  const activeRecovery: RecoveryRequest | null = useMemo(() => {
    for (const session of sessions) {
      if (session.recoveryRequest) return session.recoveryRequest;
    }
    return null;
  }, [sessions]);

  const layoutProps = {
    sessions,
    activeId: state.activeSessionId,
    onSelect: setActiveSession,
    onNewSession: () => setShowNewSession(true),
    onOpenSettings: () => setShowSettings(true),
    appVersion: update.version,
  };

  return (
    <>
      <UpdateBanner
        status={update.status}
        onInstall={update.install}
        onDismiss={update.dismiss}
      />
      {state.settings.layout === "Sidebar" && (
        <SidebarLayout {...layoutProps} />
      )}
      {state.settings.layout === "Tabs" && <TabsLayout {...layoutProps} />}
      {state.settings.layout === "Split" && <SplitLayout {...layoutProps} />}

      <NewSessionDialog
        open={showNewSession}
        onClose={() => setShowNewSession(false)}
      />
      <SettingsDialog
        open={showSettings}
        onClose={() => setShowSettings(false)}
        appVersion={update.version}
        updateStatus={update.status}
        onCheckForUpdate={update.checkForUpdate}
        onInstallUpdate={update.install}
      />

      {activeRecovery && <RecoveryDialog request={activeRecovery} />}
    </>
  );
}

export default App;
