// PROTOTYPE. Thin wrapper over the native binding: JSON in and out, AbortSignal per host call.
import { createRequire } from 'node:module';
const native = createRequire(import.meta.url)('./harness.node');

export function startSession({ turns, tools = {}, approve = () => true, deltaDelayMs = 0, eventCapacity = 64 }) {
  const inflight = new Map(); // call id -> AbortController
  // Calls and aborts travel on separate threadsafe functions, and nothing promises their order.
  // Not seen in practice; the Python binding does hit the same gap.
  // A session makes one host call at a time and ids only grow, so an abort for an id not yet
  // seen is early, and an abort for an older id is late and can be ignored.
  let lastStarted = 0;
  let earlyAbort = 0;
  const hosted = (fn) => async (callId, name, inputJson) => {
    lastStarted = callId;
    if (earlyAbort === callId) throw new Error('aborted before start');
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
    else if (callId > lastStarted) earlyAbort = callId;
  };
  const config = { turns, delta_delay_ms: deltaDelayMs, event_capacity: eventCapacity };
  const session = native.Session.start(JSON.stringify(config), callTool, ask, abort);
  return {
    async *events() {
      for (let e; (e = await session.nextEvent()) !== null; ) yield JSON.parse(e);
    },
    cancel: () => session.cancel(),
    steer: (text) => session.steer(text),
  };
}
