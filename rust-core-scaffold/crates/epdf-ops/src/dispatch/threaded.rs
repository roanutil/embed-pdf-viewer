use std::sync::mpsc::{channel, Sender};
use std::thread;

use crate::{OpsError, State};

type Job = Box<dyn FnOnce(&mut State) + Send>;

pub struct Worker {
    tx: Sender<Job>,
}

impl Worker {
    pub fn spawn() -> Result<Self, OpsError> {
        let (tx, rx) = channel::<Job>();
        let (ready_tx, ready_rx) = channel::<Result<(), OpsError>>();

        thread::Builder::new()
            .name("epdf-pdfium".to_string())
            .spawn(move || {
                let mut state = match State::new() {
                    Ok(state) => {
                        let _ = ready_tx.send(Ok(()));
                        state
                    }
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                        return;
                    }
                };
                // Ends when every Sender drops, which drops State and therefore
                // FPDF_DestroyLibrary on this thread. That is the only thread
                // allowed to call it.
                while let Ok(job) = rx.recv() {
                    job(&mut state);
                }
            })
            .map_err(|err| OpsError::Internal { message: err.to_string() })?;

        ready_rx
            .recv()
            .map_err(|err| OpsError::Internal { message: err.to_string() })??;

        Ok(Self { tx })
    }

    /// Blocks the calling thread until the PDFium thread answers. Deliberately
    /// synchronous: this scaffold does not answer the wave-0 cancellation
    /// question in docs/research/rust-core-port.md, and an async signature would
    /// imply that it does.
    pub fn run<T, F>(&self, f: F) -> Result<T, OpsError>
    where
        T: Send + 'static,
        F: FnOnce(&mut State) -> Result<T, OpsError> + Send + 'static,
    {
        let (tx, rx) = channel();
        self.tx
            .send(Box::new(move |state| {
                let _ = tx.send(f(state));
            }))
            .map_err(|_| OpsError::Closed)?;
        rx.recv().map_err(|_| OpsError::Closed)?
    }
}
