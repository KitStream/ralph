use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, watch, RwLock};
use tokio::task::JoinHandle;

use crate::events::{RecoveryAction, SessionEvent, SessionEventPayload};
use crate::session::log_store::{IterationSummary, LogRecord, SessionLogStore};
use crate::session::persist;
use crate::session::runner::run_session;
use crate::session::state::{SessionConfig, SessionId, SessionInfo, SessionStatus, SessionStep};

struct SessionHandle {
    pub info: SessionInfo,
    pub stop_tx: watch::Sender<bool>,
    pub abort_tx: watch::Sender<bool>,
    pub action_tx: Option<mpsc::Sender<RecoveryAction>>,
    pub task: Option<JoinHandle<()>>,
}

pub struct SessionManager {
    sessions: RwLock<HashMap<SessionId, SessionHandle>>,
    log_store: SessionLogStore,
    iteration_tracker: Mutex<HashMap<String, u32>>,
}

impl SessionManager {
    /// Create a new manager, loading any persisted sessions from disk.
    pub fn new() -> Self {
        let mut map = HashMap::new();

        if let Ok(persisted) = persist::load_sessions() {
            for info in persisted {
                let (stop_tx, _) = watch::channel(false);
                let (abort_tx, _) = watch::channel(false);
                map.insert(
                    info.id.clone(),
                    SessionHandle {
                        info,
                        stop_tx,
                        abort_tx,
                        action_tx: None,
                        task: None,
                    },
                );
            }
        }

        Self {
            sessions: RwLock::new(map),
            log_store: SessionLogStore::new(persist::dirs_or_default()),
            iteration_tracker: Mutex::new(HashMap::new()),
        }
    }

    async fn persist(&self) {
        let sessions = self.sessions.read().await;
        let infos: Vec<SessionInfo> = sessions.values().map(|h| h.info.clone()).collect();
        persist::save_sessions(&infos).ok();
    }

    pub async fn create_session(&self, config: SessionConfig) -> SessionId {
        let id = SessionId::new();
        let info = SessionInfo {
            id: id.clone(),
            config,
            status: SessionStatus::Created,
            last_tag: None,
            iteration_count: 0,
            ai_session_id: None,
        };

        let (stop_tx, _) = watch::channel(false);
        let (abort_tx, _) = watch::channel(false);

        let handle = SessionHandle {
            info,
            stop_tx,
            abort_tx,
            action_tx: None,
            task: None,
        };

        self.sessions.write().await.insert(id.clone(), handle);
        self.persist().await;
        id
    }

    /// Start a session fresh (no AI resume).
    pub async fn start_session(
        &self,
        id: &SessionId,
        emit: Arc<dyn Fn(SessionEvent) + Send + Sync>,
    ) -> anyhow::Result<()> {
        self.launch_session(id, emit, None, None, None).await
    }

    /// Resume an aborted session, passing the AI session ID for crash recovery.
    pub async fn resume_session(
        &self,
        id: &SessionId,
        emit: Arc<dyn Fn(SessionEvent) + Send + Sync>,
    ) -> anyhow::Result<()> {
        let (ai_session_id, resume_step, resume_iteration) = {
            let sessions = self.sessions.read().await;
            let handle = sessions
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("Session not found"))?;
            match &handle.info.status {
                SessionStatus::Aborted {
                    ai_session_id,
                    step,
                    iteration,
                } => (ai_session_id.clone(), step.clone(), *iteration),
                other => anyhow::bail!(
                    "Can only resume aborted sessions, current status: {:?}",
                    other
                ),
            }
        };

        // Log the resume attempt
        let step_info = resume_step
            .as_ref()
            .map(|s| format!(" at step {}", s))
            .unwrap_or_default();
        emit(SessionEvent {
            session_id: id.to_string(),
            payload: SessionEventPayload::Log {
                category: crate::events::LogCategory::Script,
                text: format!(
                    "Resuming session{}{}...",
                    ai_session_id
                        .as_ref()
                        .map(|s| format!(" (AI session: {})", s))
                        .unwrap_or_default(),
                    step_info,
                ),
            },
        });

        self.launch_session(id, emit, ai_session_id, resume_step, resume_iteration)
            .await
    }

    async fn launch_session(
        &self,
        id: &SessionId,
        emit: Arc<dyn Fn(SessionEvent) + Send + Sync>,
        resume_ai_session_id: Option<String>,
        resume_step: Option<SessionStep>,
        resume_iteration: Option<u32>,
    ) -> anyhow::Result<()> {
        let mut sessions = self.sessions.write().await;
        let handle = sessions
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

        // Check not already running
        match &handle.info.status {
            SessionStatus::Running { .. } | SessionStatus::Stopping { .. } => {
                anyhow::bail!("Session is already running");
            }
            _ => {}
        }

        let check_project_dir = handle.info.config.project_dir.clone();
        let check_branch_name = handle.info.config.branch_name.clone();

        for (other_id, other) in sessions.iter() {
            if other_id != id {
                if let SessionStatus::Running { .. } | SessionStatus::Stopping { .. } =
                    &other.info.status
                {
                    if other.info.config.project_dir == check_project_dir
                        && other.info.config.branch_name == check_branch_name
                    {
                        anyhow::bail!(
                            "Another session is already running on branch '{}' in '{:?}'",
                            check_branch_name,
                            check_project_dir
                        );
                    }
                }
            }
        }

        let handle = sessions.get_mut(id).unwrap();

        let (stop_tx, stop_rx) = watch::channel(false);
        let (abort_tx, abort_rx) = watch::channel(false);
        let (action_tx, action_rx) = mpsc::channel::<RecoveryAction>(1);

        let config = handle.info.config.clone();
        let session_id = id.clone();

        let task = tokio::spawn(async move {
            run_session(
                session_id,
                config,
                move |event| emit(event),
                stop_rx,
                abort_rx,
                action_rx,
                resume_ai_session_id,
                resume_step,
                resume_iteration,
            )
            .await;
        });

        handle.stop_tx = stop_tx;
        handle.abort_tx = abort_tx;
        handle.action_tx = Some(action_tx);
        handle.task = Some(task);
        handle.info.status = SessionStatus::Running {
            step: crate::session::state::SessionStep::Idle,
            iteration: 0,
        };

        drop(sessions);
        self.persist().await;
        Ok(())
    }

    pub async fn stop_session(&self, id: &SessionId) -> anyhow::Result<SessionStatus> {
        let mut sessions = self.sessions.write().await;
        let handle = sessions
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("Session not found"))?;
        handle.stop_tx.send(true).ok();
        let status = match &handle.info.status {
            SessionStatus::Running { step, iteration } => {
                let stopping = SessionStatus::Stopping {
                    step: step.clone(),
                    iteration: *iteration,
                };
                handle.info.status = stopping.clone();
                stopping
            }
            other => other.clone(),
        };
        drop(sessions);
        self.persist().await;
        Ok(status)
    }

    pub async fn cancel_stop_session(&self, id: &SessionId) -> anyhow::Result<SessionStatus> {
        let mut sessions = self.sessions.write().await;
        let handle = sessions
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("Session not found"))?;
        handle.stop_tx.send(false).ok();
        let status = match &handle.info.status {
            SessionStatus::Stopping { step, iteration } => {
                let running = SessionStatus::Running {
                    step: step.clone(),
                    iteration: *iteration,
                };
                handle.info.status = running.clone();
                running
            }
            other => other.clone(),
        };
        drop(sessions);
        self.persist().await;
        Ok(status)
    }

    /// Abort a session immediately. Returns the new `Aborted` status so the
    /// caller can emit a `StatusChanged` event to the frontend.
    pub async fn abort_session(&self, id: &SessionId) -> anyhow::Result<SessionStatus> {
        {
            let sessions = self.sessions.read().await;
            let handle = sessions
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("Session not found"))?;
            handle.stop_tx.send(true).ok();
            handle.abort_tx.send(true).ok();
        }
        // Set status to Aborted, preserving the step and iteration
        let status = {
            let mut sessions = self.sessions.write().await;
            let handle = sessions
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("Session not found"))?;
            let (step, iteration) = match &handle.info.status {
                SessionStatus::Running { step, iteration }
                | SessionStatus::Stopping { step, iteration } => {
                    (Some(step.clone()), Some(*iteration))
                }
                _ => (None, None),
            };
            let aborted = SessionStatus::Aborted {
                ai_session_id: handle.info.ai_session_id.clone(),
                step,
                iteration,
            };
            handle.info.status = aborted.clone();
            aborted
        };
        self.persist().await;
        Ok(status)
    }

    pub async fn remove_session(&self, id: &SessionId) -> anyhow::Result<()> {
        let mut sessions = self.sessions.write().await;
        if let Some(handle) = sessions.get(id) {
            if let SessionStatus::Running { .. } | SessionStatus::Stopping { .. } =
                &handle.info.status
            {
                anyhow::bail!("Cannot remove a running session. Stop it first.");
            }
        }
        let id_str = id.to_string();
        sessions.remove(id);
        drop(sessions);
        self.log_store.delete_session_logs(&id_str);
        self.persist().await;
        Ok(())
    }

    pub async fn list_sessions(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.read().await;
        sessions.values().map(|h| h.info.clone()).collect()
    }

    pub async fn get_session(&self, id: &SessionId) -> Option<SessionInfo> {
        let sessions = self.sessions.read().await;
        sessions.get(id).map(|h| h.info.clone())
    }

    pub async fn send_recovery_action(
        &self,
        id: &SessionId,
        action: RecoveryAction,
    ) -> anyhow::Result<()> {
        let sessions = self.sessions.read().await;
        let handle = sessions
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

        if let Some(tx) = &handle.action_tx {
            tx.send(action)
                .await
                .map_err(|_| anyhow::anyhow!("Session is not waiting for recovery action"))?;
        } else {
            anyhow::bail!("Session has no action channel");
        }
        Ok(())
    }

    /// Handle events from the runner to update persisted state.
    pub async fn handle_event(&self, event: &SessionEvent) {
        let id_str = &event.session_id;
        let Ok(uuid) = uuid::Uuid::parse_str(id_str) else {
            return;
        };
        let id = SessionId(uuid);

        let mut sessions = self.sessions.write().await;
        if let Some(handle) = sessions.get_mut(&id) {
            match &event.payload {
                SessionEventPayload::StatusChanged { status } => {
                    // Don't let runner's Stopped/Failed overwrite an Aborted status
                    // (abort_session sets Aborted, then the dying runner emits Stopped)
                    // But do allow Running transitions (from resume/start).
                    let dominated = matches!(
                        status,
                        SessionStatus::Stopped | SessionStatus::Failed { .. }
                    );
                    if !(dominated && matches!(handle.info.status, SessionStatus::Aborted { .. })) {
                        handle.info.status = status.clone();
                    }
                }
                SessionEventPayload::AiSessionIdChanged { ai_session_id } => {
                    handle.info.ai_session_id = ai_session_id.clone();
                }
                SessionEventPayload::IterationComplete { iteration, tag } => {
                    handle.info.iteration_count = *iteration;
                    if let Some(t) = tag {
                        handle.info.last_tag = Some(t.clone());
                    }
                }
                SessionEventPayload::Finished { .. } => {
                    // If it finished normally (not aborted), mark as Stopped
                    if !matches!(handle.info.status, SessionStatus::Aborted { .. }) {
                        handle.info.status = SessionStatus::Stopped;
                    }
                }
                _ => {}
            }
        }
        drop(sessions);

        // Append loggable events to disk
        match &event.payload {
            SessionEventPayload::Log { .. }
            | SessionEventPayload::AiContent { .. }
            | SessionEventPayload::Housekeeping { .. }
            | SessionEventPayload::RateLimited { .. }
            | SessionEventPayload::ActionRequired { .. } => {
                let iteration = {
                    let tracker = self.iteration_tracker.lock().unwrap();
                    tracker.get(id_str).copied().unwrap_or(1)
                };
                self.log_store
                    .append(id_str, iteration, &event.payload)
                    .ok();
            }
            SessionEventPayload::IterationComplete { iteration, .. } => {
                // Write the IterationComplete event to the current iteration file
                let current = {
                    let tracker = self.iteration_tracker.lock().unwrap();
                    tracker.get(id_str).copied().unwrap_or(1)
                };
                self.log_store.append(id_str, current, &event.payload).ok();
                // Advance the tracker to the next iteration
                let mut tracker = self.iteration_tracker.lock().unwrap();
                tracker.insert(id_str.to_string(), *iteration + 1);
            }
            _ => {}
        }

        self.persist().await;
    }

    pub fn list_iterations(&self, session_id: &str) -> Vec<IterationSummary> {
        self.log_store.list_iterations(session_id)
    }

    pub fn read_iteration(&self, session_id: &str, iteration: u32) -> Vec<LogRecord> {
        self.log_store.read_iteration(session_id, iteration)
    }

    pub async fn read_iteration_view(
        &self,
        session_id: &str,
        iteration: u32,
    ) -> Vec<crate::session::view::ViewLogEntry> {
        let worktree_prefix = {
            let Ok(uuid) = uuid::Uuid::parse_str(session_id) else {
                return Vec::new();
            };
            let id = SessionId(uuid);
            let sessions = self.sessions.read().await;
            sessions.get(&id).map(|h| {
                let cfg = &h.info.config;
                cfg.project_dir
                    .join(".ralph")
                    .join(format!("{}-worktree", cfg.branch_name))
                    .to_string_lossy()
                    .to_string()
            })
        };
        let prefix = worktree_prefix.as_deref().unwrap_or("");
        let records = self.log_store.read_iteration(session_id, iteration);
        crate::session::view::records_to_view_entries(&records, prefix)
    }
}
