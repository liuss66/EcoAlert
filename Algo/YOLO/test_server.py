"""Command-line smoke test for the YOLO WebSocket server."""

import argparse
import asyncio
import json
from pathlib import Path
from urllib.parse import urlsplit, urlunsplit

import cv2
import requests
import websockets


def websocket_url(server_url: str) -> str:
    value = server_url.rstrip("/")
    if "://" not in value:
        value = f"ws://{value}"
    parts = urlsplit(value)
    scheme = "wss" if parts.scheme in ("https", "wss") else "ws"
    path = parts.path if parts.path.endswith("/ws") else f"{parts.path.rstrip('/')}/ws"
    return urlunsplit((scheme, parts.netloc, path, "", ""))


def health_url(server_url: str) -> str:
    value = server_url.rstrip("/")
    if "://" not in value:
        value = f"http://{value}"
    parts = urlsplit(value)
    scheme = "https" if parts.scheme in ("https", "wss") else "http"
    return urlunsplit((scheme, parts.netloc, "/health", "", ""))


async def detect(image_path: Path, server_url: str, confidence: float) -> dict:
    image = cv2.imread(str(image_path))
    if image is None:
        raise ValueError(f"cannot read image: {image_path}")
    ok, encoded = cv2.imencode(".jpg", image)
    if not ok:
        raise ValueError(f"cannot encode image: {image_path}")
    async with websockets.connect(websocket_url(server_url), open_timeout=5) as ws:
        await ws.send(json.dumps({"type": "options", "confidence": confidence}))
        await ws.send(encoded.tobytes())
        result = json.loads(await asyncio.wait_for(ws.recv(), timeout=30))
    if result.get("error"):
        raise RuntimeError(result["error"])
    return result


async def main() -> None:
    parser = argparse.ArgumentParser(description="Test the YOLO WebSocket server")
    parser.add_argument("--url", default="ws://localhost:8090")
    parser.add_argument("--image", type=Path)
    parser.add_argument("--confidence", type=float, default=0.45)
    parser.add_argument("--health", action="store_true")
    args = parser.parse_args()

    if args.health or not args.image:
        response = requests.get(health_url(args.url), timeout=5)
        response.raise_for_status()
        print(json.dumps(response.json(), ensure_ascii=False, indent=2))
    if args.image:
        print(json.dumps(await detect(args.image, args.url, args.confidence), ensure_ascii=False, indent=2))


if __name__ == "__main__":
    asyncio.run(main())
