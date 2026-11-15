//! PROTOTYPE. napi-rs binding: host callbacks are threadsafe functions that return promises.

use std::sync::Arc;

use futures::future::BoxFuture;
use futures::FutureExt;
use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;
use proto_core::{Config, Event, Handle, Host};
use tokio::sync::{mpsc, Mutex};

// Ids cross as f64: exact up to 2^53, where u32 would wrap.
type CallArgs = FnArgs<(f64, String, String)>;
// Weak: an abandoned session must not keep the Node process alive.
type ToolFn = ThreadsafeFunction<CallArgs, Promise<String>, CallArgs, Status, false, true>;
type ApproveFn = ThreadsafeFunction<CallArgs, Promise<bool>, CallArgs, Status, false, true>;
type AbortFn = ThreadsafeFunction<f64, (), f64, Status, false, true>;

struct NodeHost {
    call_tool: Arc<ToolFn>,
    approve: Arc<ApproveFn>,
    abort: AbortFn,
}

impl Host for NodeHost {
    fn call_tool(&self, call_id: u64, name: String, input: String) -> BoxFuture<'static, std::result::Result<String, String>> {
        let f = self.call_tool.clone();
        async move {
            let promise = f.call_async((call_id as f64, name, input).into()).await.map_err(|e| e.reason.clone())?;
            promise.await.map_err(|e| e.reason.clone())
        }
        .boxed()
    }
    fn approve(&self, call_id: u64, name: String, input: String) -> BoxFuture<'static, std::result::Result<bool, String>> {
        let f = self.approve.clone();
        async move {
            let promise = f.call_async((call_id as f64, name, input).into()).await.map_err(|e| e.reason.clone())?;
            promise.await.map_err(|e| e.reason.clone())
        }
        .boxed()
    }
    fn abort(&self, call_id: u64) {
        self.abort.call(call_id as f64, ThreadsafeFunctionCallMode::NonBlocking);
    }
}

#[napi]
pub struct Session {
    handle: Handle,
    rx: Arc<Mutex<mpsc::Receiver<Event>>>,
}

#[napi]
pub struct EventReader {
    rx: Arc<Mutex<mpsc::Receiver<Event>>>,
}

#[napi]
impl EventReader {
    /// Resolves with the next event as JSON, or null when the run is over.
    #[napi]
    pub async fn next_event(&self) -> Option<String> {
        let ev = self.rx.lock().await.recv().await?;
        Some(serde_json::to_string(&ev).unwrap())
    }
}

// Dropping the session is a cancel. The event receiver alone does not show it: a pending
// next_event() holds its own reference to the receiver.
impl Drop for Session {
    fn drop(&mut self) {
        self.handle.cancel();
    }
}

#[napi]
impl Session {
    #[napi(factory)]
    pub fn start(config_json: String, call_tool: ToolFn, approve: ApproveFn, abort: AbortFn) -> Result<Session> {
        let config: Config = serde_json::from_str(&config_json).map_err(|e| Error::from_reason(e.to_string()))?;
        let host = Arc::new(NodeHost { call_tool: Arc::new(call_tool), approve: Arc::new(approve), abort });
        let (handle, rx) = within_runtime_if_available(|| proto_core::start(config, host));
        Ok(Session { handle, rx: Arc::new(Mutex::new(rx)) })
    }

    /// The event reader is a separate object. A pending read roots the object it was called
    /// on, so reading from the Session itself would keep a session with a pending read from
    /// ever being collected, and its Drop would never cancel.
    #[napi]
    pub fn reader(&self) -> EventReader {
        EventReader { rx: self.rx.clone() }
    }

    #[napi]
    pub fn cancel(&self) {
        self.handle.cancel();
    }

    #[napi]
    pub fn steer(&self, text: String) {
        self.handle.steer(text);
    }
}
