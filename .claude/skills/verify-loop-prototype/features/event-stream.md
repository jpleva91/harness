# Event stream

A session yields one ordered stream of events as an async iterator. The builder pulls events; the loop waits when the builder falls behind. The builder can steer a run between turns.

## Sub-features

- `ev-order`: events arrive in loop order and `run_finished` is last.
- `ev-steer`: a steer sent during a turn appears as `steered` between `turn_finished` and the next `turn_started`.
- `ev-retry`: transient model errors appear as `model_retry` events.
- `ev-many`: 200 sessions run at once in one process, each sees only its own tool results, and the host event loop stays responsive.
- `ev-backpressure`: with a small event capacity and no consumer, the loop stops instead of buffering.
- `ev-abandon`: a session that is never read and never cancelled does not keep the process alive.

## How to get to it (user POV)

- Node: `for await (const event of session.events())`; `session.steer(text)`; `eventCapacity` in `startSession`.
- Python: `async for event in session.events()`; `session.steer(text)`; `event_capacity` in `start_session`.

## Driving it with verify.sh

Preconditions:

- Baseline from the [index](./README.md).

- **Run both bindings.** `verify.sh drive`.
- **ev-order.** Line `PASS  async tool + async approval`; the order is listed in [host callbacks](./host-callbacks.md).
- **ev-steer.** Line `PASS  steer`. The trace above the line shows `turn_finished`, `{"type":"steered","text":"use tabs"}`, `turn_started`.
- **ev-retry.** Line `PASS  model retry events`. The trace above the line shows two `model_retry` events with attempts 1 and 2.
- **ev-many.** Line `PASS  200 concurrent sessions  max event-loop lag <n>ms`. Each session must get exactly three successful tool results carrying its own number. The scenario fails above 250 ms lag. Report the lag with the build mode.
- **ev-backpressure.** Line `PASS  backpressure  0 tool calls ran before the consumer started`.
- **ev-abandon.** The last line `<n>/<n> passed (...); exiting with an abandoned session` and `<binding>.exit` containing `0`. An exit of `124` means the abandoned session kept the process alive.

## Gotchas

- `steered` only shows that the text reached the loop. The scripted model ignores it, so nothing here proves that steering changes what a model does.
- Steering lands at turn boundaries only. A steer sent after the last turn started is never delivered, and nothing reports that.
- `ev-many` and `cb-cost` produce most of the trace file, about 12,000 event lines per binding.
- `ev-abandon` is proven only by the process exit, so it cannot be checked from a script that goes on to do other work.
