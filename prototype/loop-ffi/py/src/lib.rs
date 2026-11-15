//! PROTOTYPE. PyO3 binding: host callbacks are coroutines scheduled on the builder's asyncio loop.

use std::sync::Arc;

use futures::future::BoxFuture;
use futures::FutureExt;
use proto_core::{Config, Event, Handle, Host};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::{future_into_py, get_runtime};
use pyo3_async_runtimes::{into_future_with_locals, TaskLocals};
use tokio::sync::{mpsc, Mutex};

struct PyHost {
    call_tool: Py<PyAny>,
    approve: Py<PyAny>,
    abort: Py<PyAny>,
    locals: TaskLocals,
}

impl PyHost {
    /// Call an `async def` on the builder's loop and await its result from the tokio side.
    fn call<T>(&self, f: &Py<PyAny>, call_id: u64, name: String, input: String) -> BoxFuture<'static, Result<T, String>>
    where
        T: for<'a, 'py> FromPyObject<'a, 'py> + Send + 'static,
    {
        let fut = Python::attach(|py| {
            let coro = f.bind(py).call1((call_id, name, input))?;
            into_future_with_locals(&self.locals, coro)
        });
        async move {
            let obj = fut.map_err(|e| e.to_string())?.await.map_err(|e| e.to_string())?;
            Python::attach(|py| obj.extract::<T>(py).map_err(|_| "callback returned the wrong type".to_string()))
        }
        .boxed()
    }
}

impl Host for PyHost {
    fn call_tool(&self, call_id: u64, name: String, input: String) -> BoxFuture<'static, Result<String, String>> {
        self.call(&self.call_tool, call_id, name, input)
    }
    fn approve(&self, call_id: u64, name: String, input: String) -> BoxFuture<'static, Result<bool, String>> {
        self.call(&self.approve, call_id, name, input)
    }
    fn abort(&self, call_id: u64) {
        Python::attach(|py| {
            let _ = self.locals.event_loop(py).call_method1("call_soon_threadsafe", (self.abort.bind(py), call_id));
        });
    }
}

#[pyclass]
struct Session {
    handle: Handle,
    rx: Arc<Mutex<mpsc::Receiver<Event>>>,
}

#[pymethods]
impl Session {
    /// Must be called from inside the running asyncio loop that will run the callbacks.
    #[staticmethod]
    fn start(py: Python<'_>, config_json: &str, call_tool: Py<PyAny>, approve: Py<PyAny>, abort: Py<PyAny>) -> PyResult<Session> {
        let config: Config = serde_json::from_str(config_json).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let locals = TaskLocals::with_running_loop(py)?.copy_context(py)?;
        let host = Arc::new(PyHost { call_tool, approve, abort, locals });
        let _guard = get_runtime().enter();
        let (handle, rx) = proto_core::start(config, host);
        Ok(Session { handle, rx: Arc::new(Mutex::new(rx)) })
    }

    /// Awaitable: the next event as JSON, or None when the run is over.
    fn next_event<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let rx = self.rx.clone();
        future_into_py(py, async move {
            let ev = rx.lock().await.recv().await;
            Ok(ev.map(|e| serde_json::to_string(&e).unwrap()))
        })
    }

    fn cancel(&self) {
        self.handle.cancel();
    }

    fn steer(&self, text: String) {
        self.handle.steer(text);
    }
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Session>()
}
