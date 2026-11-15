//! PROTOTYPE. Replays the model's bug paths (Loop.tla) against the real loop.
//! Rule: once the builder has cancelled, the loop starts no new approval prompt and no new tool.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use futures::FutureExt;
use proto_core::{start, Config, Event, Host};
use tokio::sync::oneshot;

const RUNS: usize = 2000;

#[derive(Default)]
struct Probe {
    approvals_started: AtomicU32,
    tools_started: AtomicU32,
    /// Set to hold the approval open until the test answers it.
    answer: Mutex<Option<oneshot::Receiver<bool>>>,
}

impl Host for Probe {
    fn call_tool(&self, _: u64, _: String, _: String) -> BoxFuture<'static, Result<String, String>> {
        self.tools_started.fetch_add(1, Ordering::SeqCst);
        async { Ok("{}".to_string()) }.boxed()
    }
    fn approve(&self, _: u64, _: String, _: String) -> BoxFuture<'static, Result<bool, String>> {
        self.approvals_started.fetch_add(1, Ordering::SeqCst);
        match self.answer.lock().unwrap().take() {
            Some(rx) => async { rx.await.map_err(|e| e.to_string()) }.boxed(),
            None => async { Ok(true) }.boxed(),
        }
    }
    fn abort(&self, _: u64) {}
}

fn config() -> Config {
    serde_json::from_str(r#"{"turns":[{"tool_calls":[{"name":"rm"}]}]}"#).unwrap()
}

/// Model path: the builder cancels, then the loop asks for approval.
#[tokio::test]
async fn no_approval_starts_after_cancel() {
    let mut late = 0;
    for _ in 0..RUNS {
        let probe = Arc::new(Probe::default());
        let (handle, mut rx) = start(config(), probe.clone());
        handle.cancel(); // the run task has not been polled yet
        while rx.recv().await.is_some() {}
        late += probe.approvals_started.load(Ordering::SeqCst);
    }
    assert_eq!(late, 0, "{late} of {RUNS} cancelled runs still asked for approval");
}

/// Model path: approval pending, the builder cancels, the handler answers allow, the loop starts the tool.
#[tokio::test]
async fn no_tool_starts_after_cancel() {
    let mut late = 0;
    for _ in 0..RUNS {
        let (answer, held) = oneshot::channel();
        let probe = Arc::new(Probe::default());
        *probe.answer.lock().unwrap() = Some(held);
        let (handle, mut rx) = start(config(), probe.clone());
        while let Some(ev) = rx.recv().await {
            if matches!(ev, Event::ApprovalAsked { .. }) {
                break;
            }
        }
        // Wait until the loop is parked on the approval, then cancel and allow in one go.
        while probe.approvals_started.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        tokio::task::yield_now().await;
        handle.cancel();
        let _ = answer.send(true);
        while rx.recv().await.is_some() {}
        late += probe.tools_started.load(Ordering::SeqCst);
    }
    assert_eq!(late, 0, "{late} of {RUNS} cancelled runs still started the tool");
}
