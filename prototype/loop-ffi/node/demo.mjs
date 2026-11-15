// PROTOTYPE. Scenarios that push host callbacks across the FFI boundary.
import { setTimeout as sleep } from 'node:timers/promises';
import { startSession } from './harness.mjs';

const results = [];
const check = (name, ok, detail = '') => {
  results.push(Boolean(ok));
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  ' + detail : ''}`);
};
const drain = async (s, onEvent) => {
  const out = [];
  for await (const e of s.events()) {
    out.push(e);
    if (process.env.TRACE) console.log('  event', JSON.stringify(e));
    await onEvent?.(e, s);
  }
  return out;
};
const types = (evs) => evs.map((e) => e.type).join(' ');
const last = (evs) => evs.at(-1);
const call = (name, input = {}, extra = {}) => ({ name, input, ...extra });

// 1. async tool and async approval, in order
{
  const evs = await drain(startSession({
    turns: [{ text: ['hel', 'lo'], tool_calls: [call('add', { a: 2, b: 3 })] }, { text: ['done'] }],
    tools: { add: async ({ a, b }) => { await sleep(5); return { sum: a + b }; } },
    approve: async () => { await sleep(5); return true; },
  }));
  const want = 'run_started turn_started text_delta text_delta tool_call approval_asked approval_answered tool_result turn_finished turn_started text_delta turn_finished run_finished';
  const result = evs.find((e) => e.type === 'tool_result');
  check('async tool + async approval', types(evs) === want && result.ok && result.output === '{"sum":5}', result.output);
}

// 2. sync callbacks
{
  const evs = await drain(startSession({
    turns: [{ tool_calls: [call('echo', { x: 1 })] }],
    tools: { echo: (input) => input },
    approve: () => true,
  }));
  check('sync callbacks', evs.find((e) => e.type === 'tool_result')?.output === '{"x":1}');
}

// 3. tool throws: error becomes a tool result, run continues
{
  const evs = await drain(startSession({
    turns: [{ tool_calls: [call('boom'), call('echo', { ok: 1 })] }],
    tools: { boom: async () => { throw new Error('kaboom'); }, echo: (i) => i },
  }));
  const [a, b] = evs.filter((e) => e.type === 'tool_result');
  check('tool throws', !a.ok && a.output.includes('kaboom') && b.ok && last(evs).reason === 'completed', a.output);
}

// 4. approval handler throws: fail closed, tool never runs
{
  let ran = false;
  const evs = await drain(startSession({
    turns: [{ tool_calls: [call('echo')] }],
    tools: { echo: () => { ran = true; return {}; } },
    approve: async () => { throw new Error('policy service down'); },
  }));
  const ans = evs.find((e) => e.type === 'approval_answered');
  check('approval throws -> deny', !ans.allowed && !ran && ans.reason.includes('policy service down'), ans.reason);
}

// 5. deny
{
  let ran = false;
  const evs = await drain(startSession({
    turns: [{ tool_calls: [call('rm')] }],
    tools: { rm: () => { ran = true; return {}; } },
    approve: ({ name }) => name !== 'rm',
  }));
  check('deny', !ran && evs.find((e) => e.type === 'tool_result').output === 'denied');
}

// 6. cancel while a tool is running: host sees the abort, run ends promptly
{
  let aborted = false;
  const t0 = performance.now();
  let cancelledAt;
  const evs = await drain(
    startSession({
      turns: [{ tool_calls: [call('slow')] }, { text: ['never'] }],
      tools: {
        slow: (_i, signal) => new Promise((_res, rej) => {
          signal.addEventListener('abort', () => { aborted = true; rej(new Error('aborted')); });
        }),
      },
    }),
    async (e, s) => {
      if (e.type === 'approval_answered') { await sleep(20); cancelledAt = performance.now(); s.cancel(); }
    },
  );
  await sleep(5);
  const ms = performance.now() - cancelledAt;
  check('cancel mid-tool', aborted && last(evs).reason === 'cancelled' && !evs.some((e) => e.type === 'text_delta'), `${ms.toFixed(1)}ms after cancel`);
}

// 7. cancel while approval is pending
{
  let aborted = false;
  const s = startSession({
    turns: [{ tool_calls: [call('echo')] }],
    tools: { echo: (i) => i },
    approve: (_req, signal) => new Promise((_res, rej) => signal.addEventListener('abort', () => { aborted = true; rej(new Error('aborted')); })),
  });
  const evs = await drain(s, async (e) => { if (e.type === 'approval_asked') { await sleep(10); s.cancel(); } });
  await sleep(5);
  check('cancel mid-approval', aborted && last(evs).reason === 'cancelled' && !evs.some((e) => e.type === 'tool_result'));
}

// 8. core-side timeout aborts the host call, run continues
{
  let aborted = false;
  const evs = await drain(startSession({
    turns: [{ tool_calls: [call('hang', {}, { timeout_ms: 30 })] }, { text: ['after'] }],
    tools: { hang: (_i, signal) => new Promise(() => signal.addEventListener('abort', () => { aborted = true; })) },
  }));
  await sleep(5);
  const r = evs.find((e) => e.type === 'tool_result');
  check('tool timeout', aborted && !r.ok && r.output === 'timed out' && last(evs).reason === 'completed');
}

// 9. steering lands at the next turn boundary
{
  // The tool is slow on purpose: the loop runs ahead of the consumer, so a steer sent on
  // seeing an event only lands before the next turn if the loop is still busy.
  const s = startSession({ turns: [{ text: ['a'], tool_calls: [call('echo')] }, { text: ['b'] }], tools: { echo: async (i) => { await sleep(20); return i; } } });
  const evs = await drain(s, (e) => { if (e.type === 'tool_call') s.steer('use tabs'); });
  const i = evs.findIndex((e) => e.type === 'steered');
  check('steer', i > 0 && evs[i - 1].type === 'turn_finished' && evs[i + 1].type === 'turn_started' && evs[i].text === 'use tabs');
}

// 10. model retry is visible
{
  const evs = await drain(startSession({ turns: [{ transient_errors: 2, text: ['ok'] }] }));
  check('model retry events', evs.filter((e) => e.type === 'model_retry').length === 2);
}

// 11. 200 concurrent sessions, isolated, event loop stays responsive
{
  let maxLag = 0, lastTick = performance.now();
  const timer = setInterval(() => { const n = performance.now(); maxLag = Math.max(maxLag, n - lastTick - 1); lastTick = n; }, 1);
  const runs = await Promise.all(Array.from({ length: 200 }, (_, k) =>
    drain(startSession({
      turns: [{ text: ['x'], tool_calls: [call('id'), call('id'), call('id')] }],
      tools: { id: async () => { await sleep(1); return { k }; } },
      deltaDelayMs: 1,
    }))));
  clearInterval(timer);
  const ok = runs.every((evs, k) => evs.filter((e) => e.type === 'tool_result').every((r) => r.output === `{"k":${k}}`) && last(evs).reason === 'completed');
  check('200 concurrent sessions', ok, `max event-loop lag ${maxLag.toFixed(1)}ms`);
}

// 12. backpressure: a slow consumer slows the loop instead of buffering without bound
{
  let produced = 0;
  const s = startSession({
    turns: [{ tool_calls: Array.from({ length: 50 }, () => call('count')) }],
    tools: { count: () => ++produced },
    eventCapacity: 4,
  });
  await sleep(50); // nothing consumed yet
  const before = produced;
  const evs = await drain(s);
  check('backpressure', before <= 2 && produced === 50 && last(evs).reason === 'completed', `${before} tool calls ran before the consumer started`);
}

// 13. round-trip cost of a host callback
{
  const n = 2000;
  const t0 = performance.now();
  await drain(startSession({ turns: [{ tool_calls: Array.from({ length: n }, () => call('noop')) }], tools: { noop: () => 0 } }));
  const per = (performance.now() - t0) / n;
  check('callback overhead', per < 1, `${(per * 1000).toFixed(0)}us per tool call (approval + tool + 4 events)`);
}

// 14. abandoned session: never drained, never cancelled. The process must still exit.
startSession({ turns: [{ tool_calls: Array.from({ length: 500 }, () => call('noop')) }], tools: { noop: () => 0 }, eventCapacity: 1 });

const failed = results.filter((r) => !r).length;
console.log(`\n${results.length - failed}/${results.length} passed (node ${process.version}); exiting with an abandoned session`);
process.exitCode = failed ? 1 : 0;
