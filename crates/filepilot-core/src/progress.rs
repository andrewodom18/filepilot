use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use serde::{Deserialize, Serialize};

use crate::{FilePilotError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub phase: String,
    pub completed: u64,
    pub total: Option<u64>,
    pub current_path: Option<PathBuf>,
    pub message: Option<String>,
}

pub trait ProgressReporter: Send + Sync {
    fn report(&self, event: ProgressEvent);
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(FilePilotError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone)]
pub struct OperationContext {
    pub cancellation: CancellationToken,
    pub reporter: Arc<dyn ProgressReporter>,
}

impl Default for OperationContext {
    fn default() -> Self {
        Self {
            cancellation: CancellationToken::default(),
            reporter: Arc::new(NoopReporter),
        }
    }
}

impl OperationContext {
    pub fn report(&self, event: ProgressEvent) {
        self.reporter.report(event);
    }
}

struct NoopReporter;

impl ProgressReporter for NoopReporter {
    fn report(&self, _event: ProgressEvent) {}
}
