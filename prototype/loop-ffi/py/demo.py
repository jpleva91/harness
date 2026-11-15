"""PROTOTYPE. The same scenarios as node/demo.mjs, against CPython and asyncio."""
import asyncio
import json
import os
import sys
import time

sys.path.insert(0, __file__.rsplit("/", 1)[0] + "/python")
from harness_proto import start_session  # noqa: E402

results = []


def check(name, ok, detail=""):
    results.append(bool(ok))
    print(f"{'PASS' if ok else 'FAIL'}  {name}{'  ' + detail if detail else ''}")


async def drain(s, on_event=None):
    out = []
    async for e in s.events():
        out.append(e)
        if os.environ.get("TRACE"):
            print("  event", json.dumps(e, separators=(",", ":")))
        if on_event:
            await on_event(e)
    return out


def types(evs):
    return " ".join(e["type"] for e in evs)


def first(evs, t):
    return next((e for e in evs if e["type"] == t), None)


def call(name, input=None, **extra):
    return {"name": name, "input": input or {}, **extra}


async def main():
    # 1. async tool and async approval, in order
    async def add(i):
        await asyncio.sleep(0.005)
        return {"sum": i["a"] + i["b"]}

    async def slow_yes(_req):
        await asyncio.sleep(0.005)
        return True

    evs = await drain(start_session(
        [{"text": ["hel", "lo"], "tool_calls": [call("add", {"a": 2, "b": 3})]}, {"text": ["done"]}],
        tools={"add": add}, approve=slow_yes))
    want = ("run_started turn_started text_delta text_delta tool_call approval_asked approval_answered "
            "tool_result turn_finished turn_started text_delta turn_finished run_finished")
    r = first(evs, "tool_result")
    check("async tool + async approval", types(evs) == want and r["ok"] and r["output"] == '{"sum":5}', r["output"])

    # 2. sync callbacks
    evs = await drain(start_session([{"tool_calls": [call("echo", {"x": 1})]}], tools={"echo": lambda i: i}))
    check("sync callbacks", first(evs, "tool_result")["output"] == '{"x":1}')

    # 3. tool raises: error becomes a tool result, run continues
    async def boom(_i):
        raise RuntimeError("kaboom")

    evs = await drain(start_session([{"tool_calls": [call("boom"), call("echo", {"ok": 1})]}],
                                    tools={"boom": boom, "echo": lambda i: i}))
    a, b = [e for e in evs if e["type"] == "tool_result"]
    check("tool raises", not a["ok"] and "kaboom" in a["output"] and b["ok"] and evs[-1]["reason"] == "completed", a["output"])

    # 4. approval handler raises: fail closed, tool never runs
    ran = []

    async def down(_req):
        raise RuntimeError("policy service down")

    evs = await drain(start_session([{"tool_calls": [call("echo")]}], tools={"echo": lambda i: ran.append(1)}, approve=down))
    ans = first(evs, "approval_answered")
    check("approval raises -> deny", not ans["allowed"] and not ran and "policy service down" in ans["reason"], ans["reason"])

    # 5. deny
    ran = []
    evs = await drain(start_session([{"tool_calls": [call("rm")]}], tools={"rm": lambda i: ran.append(1)},
                                    approve=lambda req: req["name"] != "rm"))
    check("deny", not ran and first(evs, "tool_result")["output"] == "denied")

    # 6. cancel while a tool is running: the host task is cancelled, run ends promptly
    seen = {}

    async def slow(_i):
        try:
            await asyncio.sleep(3600)
        except asyncio.CancelledError:
            seen["cancelled"] = True
            raise

    s = start_session([{"tool_calls": [call("slow")]}, {"text": ["never"]}], tools={"slow": slow})

    async def cancel_after_approval(e):
        if e["type"] == "approval_answered":
            await asyncio.sleep(0.02)
            seen["t"] = time.perf_counter()
            s.cancel()

    evs = await drain(s, cancel_after_approval)
    ms = (time.perf_counter() - seen["t"]) * 1000
    await asyncio.sleep(0.005)
    check("cancel mid-tool", seen.get("cancelled") and ms < 100 and evs[-1]["reason"] == "cancelled" and not first(evs, "text_delta"),
          f"{ms:.1f}ms after cancel")

    # 7. cancel while approval is pending
    seen = {}

    async def never(_req):
        try:
            await asyncio.sleep(3600)
        except asyncio.CancelledError:
            seen["cancelled"] = True
            raise

    s = start_session([{"tool_calls": [call("echo")]}], tools={"echo": lambda i: i}, approve=never)

    async def cancel_on_ask(e):
        if e["type"] == "approval_asked":
            await asyncio.sleep(0.01)
            s.cancel()

    evs = await drain(s, cancel_on_ask)
    await asyncio.sleep(0.005)
    check("cancel mid-approval", seen.get("cancelled") and evs[-1]["reason"] == "cancelled" and not first(evs, "tool_result"))

    # 8. core-side timeout cancels the host call, run continues
    seen = {}
    evs = await drain(start_session([{"tool_calls": [call("slow", timeout_ms=30)]}, {"text": ["after"]}], tools={"slow": slow}))
    await asyncio.sleep(0.005)
    r = first(evs, "tool_result")
    check("tool timeout", seen.get("cancelled") and not r["ok"] and r["output"] == "timed out" and evs[-1]["reason"] == "completed")

    # 9. steering lands at the next turn boundary
    async def slow_echo(i):
        await asyncio.sleep(0.02)
        return i

    s = start_session([{"text": ["a"], "tool_calls": [call("echo")]}, {"text": ["b"]}], tools={"echo": slow_echo})

    async def steer_on_call(e):
        if e["type"] == "tool_call":
            s.steer("use tabs")

    evs = await drain(s, steer_on_call)
    i = next((k for k, e in enumerate(evs) if e["type"] == "steered"), -1)
    check("steer", i > 0 and evs[i - 1]["type"] == "turn_finished" and evs[i + 1]["type"] == "turn_started" and evs[i]["text"] == "use tabs")

    # 10. model retry is visible
    evs = await drain(start_session([{"transient_errors": 2, "text": ["ok"]}]))
    check("model retry events", sum(e["type"] == "model_retry" for e in evs) == 2)

    # 11. 200 concurrent sessions, isolated, event loop stays responsive
    lag = {"max": 0.0, "last": time.perf_counter(), "run": True}

    async def ticker():
        while lag["run"]:
            await asyncio.sleep(0.001)
            now = time.perf_counter()
            lag["max"] = max(lag["max"], (now - lag["last"] - 0.001) * 1000)
            lag["last"] = now

    tick = asyncio.create_task(ticker())

    def session(k):
        async def ident(_i):
            await asyncio.sleep(0.001)
            return {"k": k}
        return drain(start_session([{"text": ["x"], "tool_calls": [call("id")] * 3}], tools={"id": ident}, delta_delay_ms=1))

    runs = await asyncio.gather(*(session(k) for k in range(200)))
    lag["run"] = False
    await tick
    def session_ok(k, evs):
        rs = [e for e in evs if e["type"] == "tool_result"]
        return len(rs) == 3 and all(r["ok"] and r["output"] == f'{{"k":{k}}}' for r in rs) and evs[-1]["reason"] == "completed"

    ok = all(session_ok(k, evs) for k, evs in enumerate(runs))
    check("200 concurrent sessions", ok and lag["max"] < 250, f"max event-loop lag {lag['max']:.1f}ms")

    # 12. backpressure: a slow consumer slows the loop instead of buffering without bound
    produced = []
    s = start_session([{"tool_calls": [call("count")] * 50}], tools={"count": lambda i: produced.append(1) or len(produced)},
                      event_capacity=4)
    await asyncio.sleep(0.05)
    before = len(produced)
    evs = await drain(s)
    check("backpressure", before <= 2 and len(produced) == 50 and evs[-1]["reason"] == "completed",
          f"{before} tool calls ran before the consumer started")

    # 13. round-trip cost of a host callback
    n = 2000
    t0 = time.perf_counter()
    await drain(start_session([{"tool_calls": [call("noop")] * n}], tools={"noop": lambda i: 0}))
    per = (time.perf_counter() - t0) / n
    check("callback overhead", per < 0.001, f"{per * 1e6:.0f}us per tool call (approval + tool + 4 events)")

    # 14. a timeout of zero aborts the tool at the earliest moment: it must do no work
    ran = []

    async def late(_i):
        ran.append(1)
        return {}

    # Block the event loop just after the approval is answered, so the tool call and its
    # abort (timeout 0) are both queued before the tool's task takes its first step.
    async def allow_then_block(_req):
        loop = asyncio.get_running_loop()
        loop.call_soon(lambda: loop.call_soon(time.sleep, 0.05))
        return True

    evs = await drain(start_session([{"tool_calls": [call("late", timeout_ms=0)]}], tools={"late": late},
                                    approve=allow_then_block))
    await asyncio.sleep(0.01)
    check("immediate abort", not ran and first(evs, "tool_result")["output"] == "timed out")

    # 15. dropped session: the pending approval handler is cancelled
    seen = {}
    s = start_session([{"tool_calls": [call("echo")]}], tools={"echo": lambda i: i}, approve=never)
    async for e in s.events():
        if e["type"] == "approval_asked":
            break
    await asyncio.sleep(0.01)
    del s, e
    await asyncio.sleep(0.05)
    check("dropped session cancels the pending approval", seen.get("cancelled"))

    # 16. abandoned session: never drained, never cancelled. The process must still exit.
    start_session([{"tool_calls": [call("noop")] * 500}], tools={"noop": lambda i: 0}, event_capacity=1)


asyncio.run(main())
failed = results.count(False)
print(f"\n{len(results) - failed}/{len(results)} passed (python {sys.version.split()[0]}); exiting with an abandoned session")
sys.exit(1 if failed else 0)
