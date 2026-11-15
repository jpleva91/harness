// PROTOTYPE. Thin wrapper over the native binding: JSON in and out, AbortSignal per host call.
import { createRequire } from 'node:module';
const native = createRequire(import.meta.url)('./harness.node');

// The callbacks and the event iterator are built in their own scopes, away from the session.
// Closures made in one scope share one context, and the native side holds the callbacks for
// the whole run: built next to the session they would keep it alive, and a dropped session
// could never be collected and cancelled.
function makeHost(tools, approve) {
  const inflight = new Map(); // call id -> AbortController
  // Calls and aborts travel on separate threadsafe functions, and nothing promises their order.
  // Not seen in practice; the Python binding does hit the same gap.
  // Ids only grow within a session, so an abort for an id not yet seen is early and is kept
  // until that call arrives; an abort for an older id is late and is ignored.
  let lastStarted = 0;
  const earlyAborts = new Set();
  const hosted = (fn) => async (callId, name, inputJson) => {
    lastStarted = callId;
    const aborted = earlyAborts.has(callId);
    for (const id of earlyAborts) if (id <= callId) earlyAborts.delete(id);
    if (aborted) throw new Error('aborted before start');
    const ac = new AbortController();
    inflight.set(callId, ac);
    try {
      return await fn(name, JSON.parse(inputJson), ac.signal);
    } finally {
      inflight.delete(callId);
    }
  };
  const callTool = hosted(async (name, input, signal) => {
    const tool = tools[name];
    if (!tool) throw new Error(`unknown tool ${name}`);
    return JSON.stringify(await tool(input, signal));
  });
  const ask = hosted(async (name, input, signal) => Boolean(await approve({ name, input }, signal)));
  const abort = (callId) => {
    const ac = inflight.get(callId);
    if (ac) ac.abort();
    else if (callId > lastStarted) earlyAborts.add(callId);
  };
  return { callTool, ask, abort };
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
  const { callTool, ask, abort } = makeHost(tools, approve);
  const config = { turns, delta_delay_ms: deltaDelayMs, event_capacity: eventCapacity };
  const session = native.Session.start(JSON.stringify(config), callTool, ask, abort);
  return {
    events: makeEvents(session.reader()),
    cancel: () => session.cancel(),
    steer: (text) => session.steer(text),
  };
}
