//! Debounced syntax-only validation. Game code is never executed.
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: Option<usize>,
    pub message: String,
}
pub fn check(source: &str) -> Option<Diagnostic> {
    let lua = mlua::Lua::new();
    lua.load(source)
        .set_name("@BlueThinner")
        .into_function()
        .err()
        .map(|error| {
            let message = error.to_string();
            let line = message
                .split_once("BlueThinner:")
                .and_then(|(_, tail)| tail.split(':').next()?.parse().ok());
            Diagnostic { line, message }
        })
}
type Request = (u64, String, mpsc::Sender<(u64, Option<Diagnostic>)>);
fn worker() -> &'static SyncSender<Request> {
    static WORKER: std::sync::OnceLock<SyncSender<Request>> = std::sync::OnceLock::new();
    WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Request>(16);
        std::thread::spawn(move || {
            while let Ok((revision, source, reply)) = rx.recv() {
                let diagnostic = std::panic::catch_unwind(|| check(&source)).unwrap_or_else(|_| {
                    Some(Diagnostic {
                        line: None,
                        message: "Syntax checker could not inspect this source".into(),
                    })
                });
                let _ = reply.send((revision, diagnostic));
            }
        });
        tx
    })
}
#[derive(Default)]
pub struct Validation {
    source: String,
    revision: u64,
    changed_at: f64,
    submitted: bool,
    pending: Option<Receiver<(u64, Option<Diagnostic>)>>,
    pub complete: bool,
    pub diagnostic: Option<Diagnostic>,
}
impl Validation {
    pub fn tick(&mut self, source: &str, now: f64) -> bool {
        if self.revision == 0 || self.source != source {
            self.source = source.into();
            self.revision += 1;
            self.changed_at = now;
            self.submitted = false;
            self.complete = false;
            self.diagnostic = None;
        }
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok((revision, diagnostic)) => {
                    self.pending = None;
                    if revision == self.revision {
                        self.diagnostic = diagnostic;
                        self.complete = true;
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.submitted = false;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if !self.submitted && self.pending.is_none() && now - self.changed_at >= 0.4 {
            let (tx, rx) = mpsc::channel();
            match worker().try_send((self.revision, self.source.clone(), tx)) {
                Ok(()) => {
                    self.pending = Some(rx);
                    self.submitted = true;
                }
                Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Disconnected(_)) => {
                    self.diagnostic = Some(Diagnostic {
                        line: None,
                        message: "Syntax checker is unavailable".into(),
                    });
                    self.complete = true;
                    self.submitted = true;
                }
            }
        }
        !self.complete
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn syntax_only_validation_reports_line_and_never_runs_source() {
        assert!(check("error('must not execute')").is_none());
        let diagnostic = check("local x = 1\nlocal = 2").unwrap();
        assert_eq!(diagnostic.line, Some(2));
        assert!(check("function Run()\nreturn (1 + 2)\nend").is_none());
    }
    #[test]
    fn debounces_changes_and_discards_obsolete_results() {
        let mut validation = Validation::default();
        validation.tick("local = 1", 0.);
        validation.tick("local = 1", 0.2);
        assert!(validation.pending.is_none());
        validation.tick("local = 1", 0.5);
        assert!(validation.pending.is_some());
        validation.tick("local value = 1", 0.6);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !validation.complete && std::time::Instant::now() < deadline {
            validation.tick("local value = 1", 1.1);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(validation.complete);
        assert!(validation.diagnostic.is_none());
        let revision = validation.revision;
        assert!(!validation.tick("local value = 1", 3.));
        assert_eq!(validation.revision, revision);
    }
}
