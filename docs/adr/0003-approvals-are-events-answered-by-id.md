# 3. Approvals are events the builder answers by id

Date: 2026-11-15. Status: accepted. Decided in [#10](https://github.com/jpleva91/harness/issues/10).

## Context

The loop prototype ([findings](https://github.com/jpleva91/harness/blob/prototype/loop-ffi/prototype/loop-ffi/FINDINGS.md)) asked for approval through a callback the builder holds open. That works across the FFI boundary in Node and CPython, but a held-open promise or coroutine cannot survive a crash. Durable approvals have the first claim on v1 scope after the wedge ([#5](https://github.com/jpleva91/harness/issues/5)).

## Decision

The loop emits an approval request on the session's event stream, with an id, and waits. The builder answers later by id. The callback form stays in the bindings as a thin convenience over this.

- An approval waits forever by default. The builder may set a timeout on the session; an expired approval is a denial.
- In the callback convenience, a handler that fails is a denial.
- A denial goes back to the model as a tool result and the run goes on.
- After cancel, the loop starts no new approval and no new tool, and a pending approval is resolved as cancelled.

## Alternatives rejected

- **Callback only.** Simplest, and what the prototype does, but approvals die with the process, which removes durable approvals from v1.
- **Callback and event as two primitives in the core.** Two paths to specify, model and test for one behavior.
- **A default timeout.** A durable approval is safe to leave waiting; expiring it by default would deny work nobody refused.

## Consequences

- The model of approvals across crash and resume ([#14](https://github.com/jpleva91/harness/issues/14)) can treat a pending approval as session state.
- A pending approval must be part of what a session persists, so the session persistence design has to carry it.
- The event stream is per session, not per run, so a builder who resumes a session with a waiting approval has one place to read it from.
- A builder who stops reading the stream never sees the request; the run waits until they read again or cancel.
