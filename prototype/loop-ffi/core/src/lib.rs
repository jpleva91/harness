//! PROTOTYPE. The loop, driven by a scripted fake model.
//!
//! Unit of work: a *run* is one user prompt worked to completion. A run is made of
//! *turns*; a turn is one model call plus the tool calls it asked for.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// What the scripted model does in one turn.
#[derive(Debug, Clone, Deserialize)]
pub struct ScriptTurn {
    /// Text streamed as deltas, one per element.
    #[serde(default)]
    pub text: Vec<String>,
    /// Transient model errors to hit before the turn succeeds.
    #[serde(default)]
    pub transient_errors: u32,
    #[serde(default)]
    pub tool_calls: Vec<ScriptToolCall>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScriptToolCall {
    pub name: String,
    #[serde(default)]
    pub input: Value,
    /// Core-side timeout for the host callback.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub turns: Vec<ScriptTurn>,
    #[serde(default)]
    pub delta_delay_ms: u64,
    #[serde(default = "default_capacity")]
    pub event_capacity: usize,
}

fn default_capacity() -> usize {
    64
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    RunStarted,
    TurnStarted { turn: u32 },
    ModelRetry { turn: u32, attempt: u32, error: String },
    TextDelta { turn: u32, text: String },
    ToolCall { turn: u32, call_id: u64, name: String, input: Value },
    ApprovalAsked { call_id: u64, name: String, input: Value },
    ApprovalAnswered { call_id: u64, allowed: bool, reason: Option<String> },
    ToolResult { call_id: u64, ok: bool, output: String },
    Steered { text: String },
    TurnFinished { turn: u32 },
    RunFinished { reason: FinishReason },
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Completed,
    Cancelled,
}

/// The builder's callbacks. Each binding implements this over its FFI.
/// `call_id` lets the binding map a later `abort` onto the in-flight host call.
pub trait Host: Send + Sync + 'static {
    fn call_tool(&self, call_id: u64, name: String, input: String) -> BoxFuture<'static, Result<String, String>>;
    /// Returns Ok(true) to allow. An Err is treated as a denial (fail closed).
    fn approve(&self, call_id: u64, name: String, input: String) -> BoxFuture<'static, Result<bool, String>>;
    /// Tell the host that the in-flight callback for `call_id` should stop.
    fn abort(&self, call_id: u64);
}

#[derive(Clone)]
pub struct Handle {
    cancel: CancellationToken,
    steer: Arc<Mutex<Vec<String>>>,
}

impl Handle {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
    pub fn steer(&self, text: String) {
        self.steer.lock().unwrap().push(text);
    }
}

static NEXT_CALL_ID: AtomicU64 = AtomicU64::new(1);

/// Outcome of awaiting a host callback against cancellation and a timeout.
enum Hosted<T> {
    Done(Result<T, String>),
    Cancelled,
    TimedOut,
}

async fn hosted<T>(
    host: &Arc<dyn Host>,
    call_id: u64,
    fut: BoxFuture<'static, Result<T, String>>,
    cancel: &CancellationToken,
    timeout: Option<Duration>,
) -> Hosted<T> {
    let sleep = async {
        match timeout {
            Some(d) => tokio::time::sleep(d).await,
            None => std::future::pending().await,
        }
    };
    // Cancel is looked at first: a poll that begins after cancel must not take a host result
    // and go on to start the next host call (Loop.tla, NoHostCallStartsAfterCancel).
    tokio::select! {
        biased;
        _ = cancel.cancelled() => { host.abort(call_id); Hosted::Cancelled }
        r = fut => Hosted::Done(r),
        _ = sleep => { host.abort(call_id); Hosted::TimedOut }
    }
}

/// Start a run. Must be called inside a tokio runtime. Events are pulled from the
/// receiver; the channel is bounded, so a slow consumer slows the loop.
pub fn start(config: Config, host: Arc<dyn Host>) -> (Handle, mpsc::Receiver<Event>) {
    let (tx, rx) = mpsc::channel(config.event_capacity.max(1));
    let handle = Handle { cancel: CancellationToken::new(), steer: Arc::default() };
    let h = handle.clone();
    tokio::spawn(async move {
        let reason = run(config, host, &h, &tx).await.unwrap_or(FinishReason::Cancelled);
        let _ = tx.send(Event::RunFinished { reason }).await;
    });
    (handle, rx)
}

/// None means cancelled or the consumer went away.
async fn run(config: Config, host: Arc<dyn Host>, h: &Handle, tx: &mpsc::Sender<Event>) -> Option<FinishReason> {
    let cancel = &h.cancel;
    let emit = |e: Event| async move {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => None,
            r = tx.send(e) => r.ok(),
        }
    };
    emit(Event::RunStarted).await?;
    for (i, turn) in config.turns.iter().enumerate() {
        let n = i as u32 + 1;
        // Steering lands at turn boundaries only.
        let steered: Vec<String> = std::mem::take(&mut *h.steer.lock().unwrap());
        for text in steered {
            emit(Event::Steered { text }).await?;
        }
        emit(Event::TurnStarted { turn: n }).await?;
        for attempt in 1..=turn.transient_errors {
            emit(Event::ModelRetry { turn: n, attempt, error: "overloaded".into() }).await?;
        }
        for text in &turn.text {
            if config.delta_delay_ms > 0 {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return None,
                    _ = tokio::time::sleep(Duration::from_millis(config.delta_delay_ms)) => {}
                }
            }
            emit(Event::TextDelta { turn: n, text: text.clone() }).await?;
        }
        for call in &turn.tool_calls {
            let call_id = NEXT_CALL_ID.fetch_add(1, Ordering::Relaxed);
            let input = call.input.to_string();
            emit(Event::ToolCall { turn: n, call_id, name: call.name.clone(), input: call.input.clone() }).await?;
            emit(Event::ApprovalAsked { call_id, name: call.name.clone(), input: call.input.clone() }).await?;
            let fut = host.approve(call_id, call.name.clone(), input.clone());
            let (allowed, reason) = match hosted(&host, call_id, fut, cancel, None).await {
                Hosted::Done(Ok(a)) => (a, None),
                Hosted::Done(Err(e)) => (false, Some(format!("approval handler failed: {e}"))),
                Hosted::Cancelled => return None,
                Hosted::TimedOut => unreachable!(),
            };
            emit(Event::ApprovalAnswered { call_id, allowed, reason }).await?;
            if !allowed {
                emit(Event::ToolResult { call_id, ok: false, output: "denied".into() }).await?;
                continue;
            }
            let fut = host.call_tool(call_id, call.name.clone(), input);
            let timeout = call.timeout_ms.map(Duration::from_millis);
            let (ok, output) = match hosted(&host, call_id, fut, cancel, timeout).await {
                Hosted::Done(Ok(out)) => (true, out),
                Hosted::Done(Err(e)) => (false, e),
                Hosted::Cancelled => return None,
                Hosted::TimedOut => (false, "timed out".into()),
            };
            emit(Event::ToolResult { call_id, ok, output }).await?;
        }
        emit(Event::TurnFinished { turn: n }).await?;
    }
    Some(FinishReason::Completed)
}
