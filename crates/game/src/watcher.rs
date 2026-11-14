use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::{Detection, detect};

const SCAN_INTERVAL: Duration = Duration::from_secs(1);

/// Scans for the game on its own thread until dropped, reporting each change.
pub struct Watcher {
    stop: Sender<()>,
    results: Receiver<Result<Detection, String>>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    /// `wake` runs on the watcher's thread after each new result, so the caller can drain them.
    pub fn start(wake: impl Fn() + Send + 'static) -> Self {
        let (stop, stop_requests) = mpsc::channel::<()>();
        let (sender, results) = mpsc::channel();
        let thread = thread::spawn(move || {
            let mut last = None;
            loop {
                let detection = detect();
                if last.as_ref() != Some(&detection) {
                    if sender.send(detection.clone()).is_err() {
                        return;
                    }
                    wake();
                    last = Some(detection);
                }
                match stop_requests.recv_timeout(SCAN_INTERVAL) {
                    Err(RecvTimeoutError::Timeout) => continue,
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        Watcher {
            stop,
            results,
            thread: Some(thread),
        }
    }
}

impl Watcher {
    /// The results that arrived since the last call, oldest first.
    pub fn drain(&self) -> Vec<Result<Detection, String>> {
        self.results.try_iter().collect()
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // The thread may already have exited after the receiver side went away.
        if self.stop.send(()).is_err() {
            eprintln!("pececito: watcher thread already stopped");
        }
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            eprintln!("pececito: watcher thread panicked");
        }
    }
}
