//! Saves settings on a thread of their own: `Prefs::set` writes and syncs the whole file.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use rune::Waker;
use rune::prefs::Prefs;

type Write = Box<dyn FnOnce(&mut Prefs) -> Result<(), String> + Send>;

enum Request {
    Write(Write),
    /// Answers once every write sent before it is done.
    Flush(Sender<()>),
}

pub struct Settings {
    path: Option<PathBuf>,
    requests: Sender<Request>,
    results: Receiver<Result<(), String>>,
    waker: Arc<OnceLock<Waker>>,
    /// Writes sent whose results haven't been drained.
    pending: usize,
}

impl Settings {
    /// Hands `prefs` to the writer thread; read what's needed from it first.
    pub fn start(prefs: Prefs) -> Settings {
        let path = prefs.path().map(Path::to_path_buf);
        let (requests, inbox) = mpsc::channel();
        let (outbox, results) = mpsc::channel();
        let waker: Arc<OnceLock<Waker>> = Arc::default();
        let thread_waker = Arc::clone(&waker);
        thread::spawn(move || {
            let mut prefs = prefs;
            for request in inbox {
                match request {
                    Request::Write(write) => {
                        if outbox.send(write(&mut prefs)).is_err() {
                            return;
                        }
                        if let Some(waker) = thread_waker.get() {
                            waker();
                        }
                    }
                    Request::Flush(done) => {
                        if done.send(()).is_err() {
                            eprintln!("pececito: settings saved after the wait for them ended");
                        }
                    }
                }
            }
        });
        Settings {
            path,
            requests,
            results,
            waker,
            pending: 0,
        }
    }

    /// Wakes the app when a write finishes, so `poll` sees its result.
    pub fn set_waker(&self, waker: Waker) {
        if self.waker.set(waker).is_err() {
            eprintln!("pececito: the settings writer already has a waker");
        }
    }

    /// The settings file, or None when settings live only in memory.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn write(
        &mut self,
        write: impl FnOnce(&mut Prefs) -> Result<(), String> + Send + 'static,
    ) -> Result<(), String> {
        self.requests
            .send(Request::Write(Box::new(write)))
            .map_err(|_| "The settings writer stopped.".to_string())?;
        self.pending += 1;
        Ok(())
    }

    /// Results of the writes that finished since the last call, in the order they were sent.
    pub fn drain(&mut self) -> Vec<Result<(), String>> {
        let mut finished = Vec::new();
        while self.pending > 0 {
            match self.results.try_recv() {
                Ok(result) => {
                    self.pending -= 1;
                    finished.push(result);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.pending = 0;
                    finished.push(Err("The settings writer stopped unexpectedly.".into()));
                }
            }
        }
        finished
    }

    /// Waits, up to `limit`, for the writes sent so far to reach the file.
    pub fn flush(&self, limit: Duration) -> Result<(), String> {
        let (done, wait) = mpsc::channel();
        self.requests
            .send(Request::Flush(done))
            .map_err(|_| "The settings writer stopped.".to_string())?;
        wait.recv_timeout(limit)
            .map_err(|_| "Settings didn't finish saving in time.".to_string())
    }
}
