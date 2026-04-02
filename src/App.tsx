import { useState, useMemo, useEffect } from "react";
import { useSessions } from "./hooks/useSessions";
import { SidebarLayout } from "./components/layouts/SidebarLayout";
import { TabsLayout } from "./components/layouts/TabsLayout";
import { SplitLayout } from "./components/layouts/SplitLayout";
import { NewSessionDialog } from "./components/NewSessionDialog";
import { SettingsDialog } from "./components/SettingsDialog";
import { RecoveryDialog } from "./components/RecoveryDialog";
import type { SessionState, RecoveryRequest } from "./lib/types";

function App() {
  const { state, setActiveSession } = useSessions();
  const [showNewSession, setShowNewSession] = useState(false);
  const [showSettings, setShowSettings] = useState(false);

  useEffect(() => {
    document.documentElement.setAttribute(
      "data-theme",
      state.settings.theme.toLowerCase()
    );
  }, [state.settings.theme]);

  const sessions: SessionState[] = useMemo(
    () => Array.from(state.sessions.values()),
    [state.sessions]
  );

  // Find the first session with a recovery request
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
  };

  return (
    <>
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
      />

      {activeRecovery && <RecoveryDialog request={activeRecovery} />}
    </>
  );
}

export default App;
