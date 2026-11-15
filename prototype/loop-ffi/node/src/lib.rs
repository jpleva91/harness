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
type DispatchArgs = FnArgs<(String, f64, String, String)>;
// One threadsafe function carries tool calls, approval requests and aborts. Each threadsafe
// function has its own queue, so with one per kind an abort could reach JS before the call
// it aborts. One queue keeps them in the order the loop sent them.
// Weak: an abandoned session must not keep the Node process alive.
type DispatchFn = ThreadsafeFunction<DispatchArgs, Promise<String>, DispatchArgs, Status, false, true>;

struct NodeHost {
    dispatch: Arc<DispatchFn>,
}

impl NodeHost {
    fn call(&self, kind: &'static str, id: u64, name: String, input: String) -> BoxFuture<'static, std::result::Result<String, String>> {
        let f = self.dispatch.clone();
        async move {
            let promise = f.call_async((kind.to_string(), id as f64, name, input).into()).await.map_err(|e| e.reason.clone())?;
            promise.await.map_err(|e| e.reason.clone())
        }
        .boxed()
    }
}

impl Host for NodeHost {
    fn call_tool(&self, host_call: u64, name: String, input: String) -> BoxFuture<'static, std::result::Result<String, String>> {
        self.call("tool", host_call, name, input)
    }
    fn approve(&self, host_call: u64, name: String, input: String) -> BoxFuture<'static, std::result::Result<bool, String>> {
        let answer = self.call("approve", host_call, name, input);
        async move { Ok(answer.await? == "true") }.boxed()
    }
    fn abort(&self, host_call: u64) {
        let args = ("abort".to_string(), host_call as f64, String::new(), String::new()).into();
        self.dispatch.call(args, ThreadsafeFunctionCallMode::NonBlocking);
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
    pub fn start(config_json: String, dispatch: DispatchFn) -> Result<Session> {
        let config: Config = serde_json::from_str(&config_json).map_err(|e| Error::from_reason(e.to_string()))?;
        let host = Arc::new(NodeHost { dispatch: Arc::new(dispatch) });
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
