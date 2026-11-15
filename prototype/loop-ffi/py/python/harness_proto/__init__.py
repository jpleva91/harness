"""PROTOTYPE. Thin wrapper over the native binding: JSON in and out, task cancellation per host call."""
import asyncio
import inspect
import json

from . import _native


async def _maybe_await(value):
    return await value if inspect.isawaitable(value) else value


class Session:
    def __init__(self, native):
        self._native = native

    async def events(self):
        while (e := await self._native.next_event()) is not None:
            yield json.loads(e)

    def cancel(self):
        self._native.cancel()

    def steer(self, text):
        self._native.steer(text)


def start_session(turns, tools=None, approve=lambda req: True, delta_delay_ms=0, event_capacity=64):
    """Call from inside a running asyncio loop. Callbacks run on that loop."""
    tools = tools or {}
    inflight = {}  # call id -> asyncio.Task

    def hosted(fn):
        async def run(call_id, name, input_json):
            inflight[call_id] = asyncio.current_task()
            try:
                return await fn(name, json.loads(input_json))
            finally:
                inflight.pop(call_id, None)
        return run

    async def call_tool(name, input):
        return json.dumps(await _maybe_await(tools[name](input)), separators=(",", ":"))

    async def ask(name, input):
        return bool(await _maybe_await(approve({"name": name, "input": input})))

    def abort(call_id):
        task = inflight.get(call_id)
        if task:
            task.cancel()

    config = {"turns": turns, "delta_delay_ms": delta_delay_ms, "event_capacity": event_capacity}
    return Session(_native.Session.start(json.dumps(config), hosted(call_tool), hosted(ask), abort))
