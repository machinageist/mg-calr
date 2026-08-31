use std::{future::Future, pin::Pin};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod null;

pub type BackendFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, BackendError>> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentationRequest {
    pub summary: String,
    pub body: String,
    pub actions: Vec<String>,
    pub urgency: Urgency,
    pub category: String,
    pub expire_timeout_ms: i32,
    pub replaces: Option<BackendHandle>,
    pub stack_tag: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BackendHandle(pub u32);

pub trait NotificationBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn present(&self, request: PresentationRequest) -> BackendFuture<'_, BackendHandle>;
    fn close(&self, handle: BackendHandle) -> BackendFuture<'_, ()>;
}

/// Retry safety is encoded in the outer variant. Unknown outcomes must never
/// be converted into a retryable error by callers.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum BackendError {
    #[error("notification was provably not sent: {0}")]
    NotSent(NotSentCause),
    #[error("notification outcome is unknown: {0}")]
    UnknownOutcome(UnknownOutcomeCause),
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum NotSentCause {
    #[error("no notification backend is available")]
    Unavailable,
    #[error("the backend refused the request before writing it")]
    RefusedBeforeWrite,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum UnknownOutcomeCause {
    #[error("the backend reply was lost")]
    ReplyLost,
    #[error("the request was cancelled after it was written")]
    CancelledAfterWrite,
}
