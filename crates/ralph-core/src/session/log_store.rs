use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::events::SessionEventPayload;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRecord {
    pub line_no: u32,
    pub timestamp: u64,
    pub payload: SessionEventPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationSummary {
    pub iteration: u32,
    pub entry_count: u32,
}

struct WriterState {
    writer: BufWriter<File>,
    iteration: u32,
    line_count: u32,
}

pub struct SessionLogStore {
    base_dir: PathBuf,
    writers: Mutex<HashMap<String, WriterState>>,
}

impl SessionLogStore {
    pub fn new(base_dir: PathBuf) -> Self {
        let logs_dir = base_dir.join("logs");
        fs::create_dir_all(&logs_dir).ok();
        Self {
            base_dir: logs_dir,
            writers: Mutex::new(HashMap::new()),
        }
    }

    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.base_dir.join(session_id)
    }

    fn iteration_path(&self, session_id: &str, iteration: u32) -> PathBuf {
        self.session_dir(session_id)
            .join(format!("{}.jsonl", iteration))
    }

    pub fn append(
        &self,
        session_id: &str,
        iteration: u32,
        payload: &SessionEventPayload,
    ) -> std::io::Result<()> {
        let mut writers = self.writers.lock().unwrap();

        let needs_new_writer = match writers.get(session_id) {
            None => true,
            Some(state) => state.iteration != iteration,
        };

        if needs_new_writer {
            // Flush and drop old writer if exists
            if let Some(mut old) = writers.remove(session_id) {
                old.writer.flush().ok();
            }

            let dir = self.session_dir(session_id);
            fs::create_dir_all(&dir)?;

            let path = self.iteration_path(session_id, iteration);
            // Count existing lines if file already exists (resume case)
            let line_count = if path.exists() {
                let f = File::open(&path)?;
                BufReader::new(f).lines().count() as u32
            } else {
                0
            };

            let file = OpenOptions::new().create(true).append(true).open(&path)?;

            writers.insert(
                session_id.to_string(),
                WriterState {
                    writer: BufWriter::new(file),
                    iteration,
                    line_count,
                },
            );
        }

        let state = writers.get_mut(session_id).unwrap();

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let record = LogRecord {
            line_no: state.line_count,
            timestamp,
            payload: payload.clone(),
        };

        let line = serde_json::to_string(&record)?;
        writeln!(state.writer, "{}", line)?;
        state.writer.flush()?;
        state.line_count += 1;

        Ok(())
    }

    pub fn read_iteration(&self, session_id: &str, iteration: u32) -> Vec<LogRecord> {
        let path = self.iteration_path(session_id, iteration);
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        BufReader::new(file)
            .lines()
            .filter_map(|line| line.ok())
            .filter_map(|line| serde_json::from_str::<LogRecord>(&line).ok())
            .collect()
    }

    pub fn list_iterations(&self, session_id: &str) -> Vec<IterationSummary> {
        let dir = self.session_dir(session_id);
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };

        let mut summaries: Vec<IterationSummary> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let iteration: u32 = name.strip_suffix(".jsonl")?.parse().ok()?;
                let file = File::open(e.path()).ok()?;
                let entry_count = BufReader::new(file).lines().count() as u32;
                Some(IterationSummary {
                    iteration,
                    entry_count,
                })
            })
            .collect();

        summaries.sort_by_key(|s| s.iteration);
        summaries
    }

    pub fn delete_session_logs(&self, session_id: &str) {
        // Remove writer if open
        {
            let mut writers = self.writers.lock().unwrap();
            writers.remove(session_id);
        }
        let dir = self.session_dir(session_id);
        fs::remove_dir_all(&dir).ok();
    }

    pub fn flush(&self, session_id: &str) {
        let mut writers = self.writers.lock().unwrap();
        if let Some(state) = writers.get_mut(session_id) {
            state.writer.flush().ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::LogCategory;

    fn log_payload(text: &str) -> SessionEventPayload {
        SessionEventPayload::Log {
            category: LogCategory::Script,
            text: text.to_string(),
        }
    }

    /// A new log_store with no prior in-memory state must still route an
    /// append to the iteration named by the caller — never a fallback like
    /// "1". This is the regression case: on app restart, the manager hands
    /// the store the iteration from the event stamp and we must respect it.
    #[test]
    fn append_to_iteration_after_cold_start_routes_by_caller() {
        let temp = tempfile::tempdir().unwrap();
        let store = SessionLogStore::new(temp.path().to_path_buf());
        let session = "session-cold";

        // No prior state; the next iteration we emit is 6 (e.g. resumed from
        // disk after a crash mid-iteration 6).
        store.append(session, 6, &log_payload("first")).unwrap();
        store.append(session, 6, &log_payload("second")).unwrap();

        let recs_6 = store.read_iteration(session, 6);
        assert_eq!(recs_6.len(), 2, "events must land in iteration 6");
        assert!(
            store.read_iteration(session, 1).is_empty(),
            "no event should leak into iteration 1 (the old fallback)"
        );
    }

    #[test]
    fn append_creates_one_file_per_iteration() {
        let temp = tempfile::tempdir().unwrap();
        let store = SessionLogStore::new(temp.path().to_path_buf());
        let s = "session-multi";

        store.append(s, 1, &log_payload("a")).unwrap();
        store.append(s, 1, &log_payload("b")).unwrap();
        store.append(s, 2, &log_payload("c")).unwrap();
        store.append(s, 4, &log_payload("d")).unwrap();

        let summaries = store.list_iterations(s);
        let counts: Vec<(u32, u32)> = summaries
            .iter()
            .map(|s| (s.iteration, s.entry_count))
            .collect();
        assert_eq!(counts, vec![(1, 2), (2, 1), (4, 1)]);
    }

    /// Re-opening an existing iteration file appends rather than overwriting,
    /// so a resumed iteration's events land alongside the events written
    /// before the restart. This guards the fact that we never destroy prior
    /// records when re-routing into a known iteration.
    #[test]
    fn append_to_existing_iteration_preserves_prior_records() {
        let temp = tempfile::tempdir().unwrap();
        let s = "session-resume";

        // Pre-populate iteration 3 with two records.
        {
            let store = SessionLogStore::new(temp.path().to_path_buf());
            store.append(s, 3, &log_payload("pre-1")).unwrap();
            store.append(s, 3, &log_payload("pre-2")).unwrap();
        }

        // Cold-start (drop and recreate the store) and append two more.
        {
            let store = SessionLogStore::new(temp.path().to_path_buf());
            store.append(s, 3, &log_payload("post-1")).unwrap();
            store.append(s, 3, &log_payload("post-2")).unwrap();
        }

        let recs = SessionLogStore::new(temp.path().to_path_buf()).read_iteration(s, 3);
        assert_eq!(recs.len(), 4, "all four records must be preserved");
    }
}
