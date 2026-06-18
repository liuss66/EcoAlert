"""WebSocket server for YOLO person detection."""
import json
import asyncio
import atexit
import secrets
import time
import yaml
import os
import cv2
import numpy as np
from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
import uvicorn

from yolo_detector import YOLODetector

# Load config
BASE_DIR = os.path.dirname(os.path.abspath(__file__))
config_path = os.path.join(BASE_DIR, "config.yaml")
with open(config_path, "r", encoding="utf-8") as f:
    config = yaml.safe_load(f)

model_config = config.get("model", {})
server_config = config.get("server", {})
max_frame_bytes = int(server_config.get("max_frame_bytes", 5 * 1024 * 1024))
queue_timeout_sec = float(server_config.get("queue_timeout_sec", 2.0))
auth_token = os.environ.get(
    "ECOALERT_YOLO_TOKEN", str(server_config.get("auth_token", ""))
).strip()
inference_slot = asyncio.Semaphore(1)

# Resolve relative weight paths against this module, not the caller's cwd.  The
# desktop app and release scripts normally start the server from another folder.
model_name = model_config.get("name", "model/yolo11s.pt")
if not os.path.isabs(model_name):
    model_name = os.path.join(BASE_DIR, model_name)

# Initialize detector
print("Loading model...")
detector = YOLODetector(
    model_name=model_name,
    confidence=model_config.get("confidence", 0.45),
    iou_threshold=model_config.get("iou_threshold", 0.35),
    classes=model_config.get("classes", [0]),
    device=model_config.get("device", "auto"),
    imgsz=model_config.get("imgsz", 640)
)
print(f"Model loaded on {detector.device}")
atexit.register(detector.close)

# Create FastAPI app
app = FastAPI(title="YOLO WebSocket Server")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


@app.get("/health")
async def health():
    return {
        "status": "healthy",
        "model": detector.model_name,
        "device": detector.device,
        "max_frame_bytes": max_frame_bytes,
    }


@app.websocket("/ws")
async def websocket_endpoint(websocket: WebSocket):
    """WebSocket endpoint"""
    supplied_token = websocket.query_params.get("token", "")
    if auth_token and not secrets.compare_digest(supplied_token, auth_token):
        await websocket.close(code=1008, reason="invalid token")
        return
    await websocket.accept()
    print("[Server] Client connected")
    confidence = detector.confidence

    try:
        while True:
            try:
                message = await websocket.receive()
                if message.get("type") == "websocket.disconnect":
                    break
                if message.get("text") is not None:
                    options = json.loads(message["text"])
                    if options.get("type") != "options":
                        raise ValueError("unsupported text message")
                    confidence = min(1.0, max(0.0, float(options["confidence"])))
                    continue
                data = message.get("bytes")
                if data is None:
                    raise ValueError("binary JPEG frame required")
                if len(data) > max_frame_bytes:
                    raise ValueError(f"frame too large ({len(data)} > {max_frame_bytes})")
            except (ValueError, KeyError, TypeError, json.JSONDecodeError) as e:
                await websocket.send_text(json.dumps({"error": str(e)}))
                continue

            try:
                nparr = np.frombuffer(data, np.uint8)
                img = cv2.imdecode(nparr, cv2.IMREAD_COLOR)
                if img is None:
                    raise ValueError("invalid JPEG image")

                # Ultralytics inference is synchronous. Run it off the asyncio
                # event loop so health checks and other clients remain usable.
                start = time.time()
                try:
                    await asyncio.wait_for(inference_slot.acquire(), timeout=queue_timeout_sec)
                except asyncio.TimeoutError:
                    await websocket.send_text(json.dumps({"error": "server busy"}))
                    continue
                try:
                    detections, _ = await detector.detect_async(img, confidence=confidence)
                finally:
                    inference_slot.release()
                process_ms = (time.time() - start) * 1000
                result = {
                    "count": len(detections),
                    "detections": detections,
                    "process_ms": round(process_ms, 2),
                    "confidence": confidence,
                }
                await websocket.send_text(json.dumps(result))
            except Exception as e:
                # Keep the long-lived connection usable and give the Rust
                # client the real inference/decode error.
                print(f"[Server] Frame processing error: {e}")
                await websocket.send_text(json.dumps({"error": str(e)}))

    except WebSocketDisconnect:
        print("[Server] Client disconnected")
    except Exception as e:
        print(f"[Server] Error: {e}")
        import traceback
        traceback.print_exc()
    finally:
        try:
            await websocket.close()
        except:
            pass


if __name__ == "__main__":
    host = server_config.get("host", "127.0.0.1")
    port = server_config.get("port", 8090)
    if host not in {"127.0.0.1", "localhost", "::1"} and not auth_token:
        raise RuntimeError(
            "ECOALERT_YOLO_TOKEN or server.auth_token is required for non-loopback binding"
        )
    print(f"Starting server on {host}:{port}")
    uvicorn.run(app, host=host, port=port, ws_max_size=max_frame_bytes)
