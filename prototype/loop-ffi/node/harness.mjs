// PROTOTYPE. Thin wrapper over the native binding: JSON in and out, AbortSignal per host call.
import { createRequire } from 'node:module';
const native = createRequire(import.meta.url)('./harness.node');

// The callbacks and the event iterator are built in their own scopes, away from the session.
// Closures made in one scope share one context, and the native side holds the callbacks for
// the whole run: built next to the session they would keep it alive, and a dropped session
// could never be collected and cancelled.
function makeHost(tools, approve) {
  const inflight = new Map(); // host call id -> AbortController
  const hosted = async (id, fn) => {
    const ac = new AbortController();
    inflight.set(id, ac);
    try {
      return await fn(ac.signal);
    } finally {
      inflight.delete(id);
    }
  };
  // One entry point for calls and aborts: the native side sends them down one queue, so an
  // abort always arrives after the call it aborts, and an abort for a finished call finds nothing.
  return (kind, id, name, inputJson) => {
    if (kind === 'abort') {
      inflight.get(id)?.abort();
      return Promise.resolve('');
    }
    const input = JSON.parse(inputJson);
    if (kind === 'approve') return hosted(id, async (signal) => String(Boolean(await approve({ name, input }, signal))));
    return hosted(id, async (signal) => {
      const tool = tools[name];
      if (!tool) throw new Error(`unknown tool ${name}`);
      return JSON.stringify(await tool(input, signal));
    });
  };
}

// The iterator holds only the reader, so a pending read does not keep the session alive.
// It is handed out through an arrow: called as a method, a generator keeps its receiver,
// which is the object that holds the session.
function makeEvents(reader) {
  async function* events() {
    for (let e; (e = await reader.nextEvent()) !== null; ) yield JSON.parse(e);
  }
  return () => events();
}

export function startSession({ turns, tools = {}, approve = () => true, deltaDelayMs = 0, eventCapacity = 64 }) {
  const dispatch = makeHost(tools, approve);
  const config = { turns, delta_delay_ms: deltaDelayMs, event_capacity: eventCapacity };
  const session = native.Session.start(JSON.stringify(config), dispatch);
  return {
    events: makeEvents(session.reader()),
    cancel: () => session.cancel(),
    steer: (text) => session.steer(text),
  };
}
