"""WebSocket smoke test for riptheGamePad's AI input server.

Connects to ws://127.0.0.1:7777 (the default in config.default.toml) and
sends a sequence of frames exercising every Frame variant the server
recognizes: hello, press, release, axis, trigger.

Usage:
    python scripts/ws-smoke.py
    python scripts/ws-smoke.py --addr ws://127.0.0.1:7777 --client smoke
"""

import argparse
import asyncio
import json
import sys

import websockets


SEQUENCE = [
    # Identify ourselves; this MUST be the first frame.
    ({"type": "hello", "client_id": "smoke-test"}, 0.05),
    # Face buttons
    ({"type": "press", "button": "South", "duration_ms": 400}, 0.2),
    ({"type": "press", "button": "East", "duration_ms": 200}, 0.2),
    ({"type": "press", "button": "North", "duration_ms": 200}, 0.2),
    ({"type": "press", "button": "West", "duration_ms": 200}, 0.2),
    # DPad
    ({"type": "press", "button": "DPadUp", "duration_ms": 200}, 0.2),
    # Sticks
    ({"type": "axis", "axis": "LeftStickX", "value": -0.7}, 0.3),
    ({"type": "axis", "axis": "LeftStickX", "value": 0.0}, 0.05),
    ({"type": "axis", "axis": "RightStickY", "value": 0.5}, 0.3),
    ({"type": "axis", "axis": "RightStickY", "value": 0.0}, 0.05),
    # Triggers
    ({"type": "trigger", "trigger": "R2", "value": 1.0}, 0.3),
    ({"type": "trigger", "trigger": "R2", "value": 0.0}, 0.05),
    ({"type": "trigger", "trigger": "L2", "value": 0.7}, 0.3),
    ({"type": "trigger", "trigger": "L2", "value": 0.0}, 0.05),
    # Manual press / release pair (no auto-release)
    ({"type": "press", "button": "Start", "duration_ms": 100}, 0.15),
]


async def run(addr: str, client_id: str | None) -> int:
    if client_id:
        SEQUENCE[0] = ({"type": "hello", "client_id": client_id}, 0.05)
    print(f"connecting to {addr}")
    try:
        async with websockets.connect(addr) as ws:
            print("connected")
            for i, (frame, delay) in enumerate(SEQUENCE):
                payload = json.dumps(frame)
                await ws.send(payload)
                summary = frame.get("button") or frame.get("axis") or frame.get("trigger") or frame.get("client_id")
                print(f"  [{i:>2}] {frame['type']:<8} {summary}")
                await asyncio.sleep(delay)
            print("sent all frames; closing")
        return 0
    except (ConnectionRefusedError, OSError) as e:
        print(f"connect failed: {e}", file=sys.stderr)
        print("is rgp running? (rgp / rgp-debug)", file=sys.stderr)
        return 1
    except websockets.exceptions.WebSocketException as e:
        print(f"websocket error: {e}", file=sys.stderr)
        return 1


def main() -> int:
    p = argparse.ArgumentParser(description="riptheGamePad WS smoke test")
    p.add_argument("--addr", default="ws://127.0.0.1:7777", help="WebSocket URL")
    p.add_argument("--client", default=None, help="Override client_id (default: smoke-test)")
    args = p.parse_args()
    return asyncio.run(run(args.addr, args.client))


if __name__ == "__main__":
    sys.exit(main())
