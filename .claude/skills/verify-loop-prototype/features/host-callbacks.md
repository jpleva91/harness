# Host callbacks

The builder hands a session a map of tools and one approval handler, written in the host language. The loop, which runs in Rust, asks the approval handler before every tool call and runs the tool only on allow.

## Sub-features

- `cb-async`: an async tool and an async approval handler are awaited across the FFI boundary and their values come back.
- `cb-sync`: plain synchronous callbacks work the same way.
- `cb-tool-error`: a tool that throws becomes a failed tool result; the run goes on.
- `cb-approval-error`: an approval handler that throws is a denial; the tool never runs.
- `cb-deny`: a denial yields the tool result `denied`; the tool never runs.
- `cb-cost`: the round trip per tool call is measured.

## How to get to it (user POV)

- Node: `startSession({ turns, tools: { name: fn }, approve: fn })`, then `for await (const event of session.events())`.
- Python, inside a running asyncio loop: `start_session(turns, tools={"name": fn}, approve=fn)`, then `async for event in session.events()`.

## Driving it with verify.sh

Preconditions:

- Baseline from the [index](./README.md).

- **Run both bindings.** `verify.sh drive`. Each binding ends with `15/15 passed` and exit 0.
- **cb-async.** Line `PASS  async tool + async approval  {"sum":5}`. In the trace, the events above it are, in order: `run_started turn_started text_delta text_delta tool_call approval_asked approval_answered tool_result turn_finished turn_started text_delta turn_finished run_finished`, and `tool_result` has `"ok":true,"output":"{\"sum\":5}"`.
- **cb-sync.** Line `PASS  sync callbacks`.
- **cb-tool-error.** Line `PASS  tool throws  kaboom` (Python: `tool raises  RuntimeError: kaboom`). The trace above the line shows a `tool_result` with `"ok":false`, then a second `tool_result` with `"ok":true`, then `run_finished` with `"reason":"completed"`.
- **cb-approval-error.** Line `PASS  approval throws -> deny  approval handler failed: policy service down` (Python adds `RuntimeError:`). The trace above the line shows `approval_answered` with `"allowed":false`. The scenario also checks a host-side flag that the tool did not run.
- **cb-deny.** Line `PASS  deny`. The trace above the line shows `tool_result` with `"output":"denied"`.
- **cb-cost.** Line `PASS  callback overhead  <n>us per tool call (approval + tool + 4 events)`. Report the number with the build mode (release).

## Gotchas

- Tool input and output cross the boundary as JSON strings. A tool that returns something JSON cannot encode fails as a tool error.
- The Python session must be started inside the asyncio loop that will run the callbacks. Started outside one, it raises.
- Error text differs by binding: Python prefixes the exception class.
- `cb-cost` has a threshold of 1 ms per call. A debug build or a loaded machine can cross it; that is not a correctness failure.
