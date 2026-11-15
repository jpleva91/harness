# Findings: loop prototype ([#10](https://github.com/jpleva91/harness/issues/10))

Throwaway prototype. Measured on Linux x64, Rust 1.96, Node 26.13, CPython 3.12, release build, scripted model.

## Question 1: do host callbacks work across the FFI boundary?

**Yes, in both Node and CPython, with async callbacks and cancellation.** The same 15 scenarios pass in both bindings (`./run.sh`).

| What | Node (napi-rs 3) | CPython (PyO3 0.30) |
| --- | --- | --- |
| Async tool and async approval handler awaited by the Rust loop | yes | yes |
| Sync callbacks | yes | yes |
| Callback throws: becomes a tool error or a denial | yes | yes |
| Cancel reaches the host callback in flight | `AbortSignal` | task cancellation (`CancelledError`) |
| Core-side tool timeout aborts the host callback | yes | yes |
| 200 sessions at once, isolated; worst host event-loop lag | 6 ms | 16 ms |
| Cost per tool call (approval + tool + 4 events) | about 105 µs | about 390 µs |
| Process exits with an abandoned session | yes | yes |

What the bindings needed, which the real design should keep:

- **An id per host call and an abort callback.** Neither runtime lets Rust cancel a host promise or coroutine directly. The loop gives every callback invocation its own id; the binding keeps a map from id to `AbortController` (Node) or `asyncio.Task` (Python); the loop calls `abort(id)`. About 20 lines per binding.
- **An abort can overtake the call it aborts.** The call and the abort reach the host event loop separately. In Python the abort arrived before the callback's first step and was lost, so a timed-out tool ran anyway (found in review, reproduced, fixed by remembering early aborts). Node has the same gap on paper and the same guard; it was not seen to happen, so the Node guard is unproven.
- **A dropped session must count as cancel.** Otherwise a pending approval and the Rust task stay alive with nobody reading. Dropping the session now cancels the run. Python drops at once. Node drops only when the garbage collector gets to it, so a Node builder needs an explicit `cancel()` or close; and it took three fixes before the Node session could be collected at all: the event reader had to be a separate native object, the callbacks had to be built in a scope that does not contain the session, and the event generator had to stop holding its receiver. The real TypeScript binding needs a test for this.
- **Callbacks run on the builder's event loop**, not on a Rust thread. In Python the session must be started inside the running asyncio loop, which it captures.
- **Weak callback handles in Node.** With strong ones, a session that is never drained keeps the process alive.
- **Events are pulled.** `next_event()` is an awaitable on the Rust channel, wrapped as an async iterator. That gives backpressure for free: with no consumer the loop stops after filling a small buffer.
- Payloads crossed as JSON strings. Good enough for a prototype; typed payloads are a later question.

## A bug the model found

The first version raced cancel against other work with an unbiased `select!`. The TLA+ model found, and replay tests confirmed in the real loop, that **a cancelled run could still ask for approval (126 of 2000 runs) or start a tool (702 of 2000)**. Fix: every select looks at cancel first. The rule for the spec: *once the builder has cancelled, the loop starts no new approval prompt and no new tool.* A cancel that lands in the middle of a loop step can still let one call start; it is aborted at once.

More bugs came from the Codex review, all described above: the lost early abort in Python, the session dropped during a pending callback, and the Node session that could never be collected.

Checked by `.claude/skills/formal-verify-loop/check.sh` (model, mutants, replay tests) and `.claude/skills/verify-loop-prototype/verify.sh drive` (both bindings, with evidence).

## Things a builder would trip on

- **The loop runs ahead of the consumer.** By the time a builder sees `tool_call`, the tool may be done. Steering or cancelling "on seeing an event" is always late by up to the buffer size.
- **Approvals are a callback, not an event the builder answers.** `approval_asked` in the stream is information only. A builder who wants to answer from a UI holds the handler's promise open. This is the shape that must change for durable approvals: a promise cannot survive a crash.
- **A handler that never answers parks the run forever.** There is no approval timeout.
- **Steering lands only between turns**, and a steer after the last turn starts is dropped silently.
- **An abandoned session parks its loop** for as long as the session object is alive.

## Question 2: the loop unit and event stream, to react to

What the prototype sketched:

- **Run**: one prompt worked to completion or cancel. **Turn**: one model call plus the tool calls it asked for.
- One ordered, pulled stream per run: `run_started`, `turn_started`, `model_retry`, `text_delta`, `tool_call`, `approval_asked`, `approval_answered`, `tool_result`, `steered`, `turn_finished`, `run_finished {completed | cancelled}`.
- Tool failures and denials go back to the model as tool results; the run goes on. An approval handler that fails is a denial.
- `cancel()` and `steer(text)` on the session.

## Not shown

- No real model provider, no streaming from one, no retries that wait. `steered` and `model_retry` are scripted events; nothing shows steering changing a model's input or a provider call being cancelled.
- A callback that blocks its thread cannot be interrupted, in either language. A Node callback that ignores its `AbortSignal` keeps running.
- One host event loop per process. Node worker threads and several asyncio loops were not tried.
- Isolation between sessions was checked only by each session getting its own tool results back.
- No parallel tool calls. No sandboxed tool execution.
- No persistence, resume or durable approvals.
- No packaging: built with `cargo build` on one machine, not as npm or PyPI packages, not on macOS or arm64.
- Free-threaded CPython, Bun and Deno were not tried.
- No memory or leak measurement over long runs.
