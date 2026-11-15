# Cancellation and timeouts

The builder can cancel a run at any moment. The loop stops, ends the event stream with `run_finished` and reason `cancelled`, and tells the host callback in flight to stop: an `AbortSignal` in Node, task cancellation in Python. A tool can also carry a timeout set on the core side.

## Sub-features

- `cancel-tool`: cancel while a tool runs. The tool sees the abort, no later event appears, the run ends as cancelled.
- `cancel-approval`: cancel while the approval handler is pending. The handler sees the abort, the tool never runs.
- `timeout-tool`: a tool that outlives its timeout is aborted, its result is `timed out`, and the run goes on.
- `abort-immediate`: an abort sent the moment a tool is called still reaches it, so the tool does no work.
- `drop-session`: dropping the session, even with a read still pending, cancels the approval handler that is pending.

## How to get to it (user POV)

- Node: `session.cancel()`. The tool and the approval handler receive an `AbortSignal` as their last argument.
- Python: `session.cancel()`. The callback's task is cancelled, so its `await` raises `asyncio.CancelledError`.
- Timeout: `timeout_ms` on a scripted tool call. A real API would put this in session config.

## Driving it with verify.sh

Preconditions:

- Baseline from the [index](./README.md).

- **Run both bindings.** `verify.sh drive`.
- **cancel-tool.** Line `PASS  cancel mid-tool  <n>ms after cancel`. In the trace the events above it end with `approval_answered` then `run_finished` with `"reason":"cancelled"`: no `tool_result` and no `text_delta` from the second turn. The scenario checks a host-side flag that the tool saw the abort.
- **cancel-approval.** Line `PASS  cancel mid-approval`. The trace above the line ends with `approval_asked` then `run_finished` with `"reason":"cancelled"`.
- **timeout-tool.** Line `PASS  tool timeout`. The trace above the line shows `tool_result` with `"ok":false,"output":"timed out"`, then the second turn's `text_delta`, then `"reason":"completed"`.
- **abort-immediate.** Line `PASS  immediate abort`. The trace above the line shows `tool_result` with `"output":"timed out"`. The scenario blocks the host event loop so that the call and its abort are queued together, and checks a host-side flag that the tool did no work.
- **drop-session.** Line `PASS  dropped session cancels the pending approval`. The proof is a host-side flag set when the handler is cancelled. A `SKIP` line in Node means it ran without `--expose-gc` and proves nothing.
- **Every ordering.** These scenarios cancel at one chosen moment each. For all orderings of cancel against approvals and tools, run `.claude/skills/formal-verify-loop/check.sh`.

## Gotchas

- The loop runs ahead of the event consumer. A cancel sent on seeing an event can arrive after the loop has moved on, so the scenarios hold the callback open before they cancel.
- A host callback that ignores the abort keeps running in the builder's process. The loop does not wait for it.
- A cancel that lands in the middle of a loop step is seen by the next step, so one callback can start and be aborted at once.
- `ms after cancel` includes the scenario's own 5 ms settle sleep in Node. The scenario fails above 100 ms.
- `timeout-tool` in Node proves the tool was told to stop, not that it stopped: its promise never settles. Node cannot stop a callback that ignores the signal, and neither binding can interrupt a callback that blocks the thread.
- `abort-immediate` discriminates in Python only: it fails there without the wrapper's early-abort guard. In Node the call was always delivered before its abort on the test machine, so the scenario passes with or without the guard. The Node guard is unproven.
- `drop-session` leaves a native read pending on purpose, because a pending read keeps the event receiver alive; the scenario fails if the session's drop does not cancel.
- In Node a session is dropped only when the garbage collector frees it, which the scenario forces. In a real program that can be much later, or never before exit. The scenario also depends on the wrapper not holding the session from any long-lived closure: `node/harness.mjs` says how at the top.
