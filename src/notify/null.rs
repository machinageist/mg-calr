use std::{collections::VecDeque, sync::Mutex};

use super::{BackendError, BackendFuture, BackendHandle, NotificationBackend, PresentationRequest};

/// Recording backend for deterministic application and PostgreSQL tests.
#[derive(Debug, Default)]
pub struct NullBackend {
    state: Mutex<NullBackendState>,
}

#[derive(Debug, Default)]
struct NullBackendState {
    calls: Vec<PresentationRequest>,
    rendered: Vec<PresentationRequest>,
    outcomes: VecDeque<Result<(), BackendError>>,
    closed: Vec<BackendHandle>,
}

impl NullBackend {
    #[must_use]
    pub fn with_outcomes(outcomes: impl IntoIterator<Item = Result<(), BackendError>>) -> Self {
        Self {
            state: Mutex::new(NullBackendState {
                outcomes: outcomes.into_iter().collect(),
                ..NullBackendState::default()
            }),
        }
    }

    #[must_use]
    ///
    /// # Panics
    ///
    /// Panics if another thread has poisoned the backend mutex.
    pub fn calls(&self) -> Vec<PresentationRequest> {
        self.state
            .lock()
            .expect("null backend lock poisoned")
            .calls
            .clone()
    }

    #[must_use]
    ///
    /// # Panics
    ///
    /// Panics if another thread has poisoned the backend mutex.
    pub fn rendered(&self) -> Vec<PresentationRequest> {
        self.state
            .lock()
            .expect("null backend lock poisoned")
            .rendered
            .clone()
    }

    #[must_use]
    ///
    /// # Panics
    ///
    /// Panics if another thread has poisoned the backend mutex.
    pub fn closed(&self) -> Vec<BackendHandle> {
        self.state
            .lock()
            .expect("null backend lock poisoned")
            .closed
            .clone()
    }
}

impl NotificationBackend for NullBackend {
    fn name(&self) -> &'static str {
        "null"
    }

    fn present(&self, request: PresentationRequest) -> BackendFuture<'_, BackendHandle> {
        Box::pin(async move {
            let mut state = self.state.lock().expect("null backend lock poisoned");
            state.calls.push(request.clone());
            if let Some(outcome) = state.outcomes.pop_front() {
                outcome?;
            }
            state.rendered.push(request);
            let handle = u32::try_from(state.rendered.len()).unwrap_or(u32::MAX);
            Ok(BackendHandle(handle))
        })
    }

    fn close(&self, handle: BackendHandle) -> BackendFuture<'_, ()> {
        Box::pin(async move {
            self.state
                .lock()
                .expect("null backend lock poisoned")
                .closed
                .push(handle);
            Ok(())
        })
    }
}
