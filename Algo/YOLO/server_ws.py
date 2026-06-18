"""WebSocket server for YOLO person detection."""
import json
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
    return {"status": "healthy", "model": detector.model_name, "device": detector.device}


@app.websocket("/ws")
async def websocket_endpoint(websocket: WebSocket):
    """WebSocket endpoint"""
    await websocket.accept()
    print("[Server] Client connected")

    try:
        while True:
            # Try to receive bytes
            try:
                print("[Server] Waiting for image data...")
                data = await websocket.receive_bytes()
                print(f"[Server] Received {len(data)} bytes of image data")
            except Exception as e:
                print(f"[Server] Receive error: {e}")
                import traceback
                traceback.print_exc()
                break

            try:
                print(f"[Server] Decoding JPEG image ({len(data)} bytes)...")
                nparr = np.frombuffer(data, np.uint8)
                img = cv2.imdecode(nparr, cv2.IMREAD_COLOR)
                if img is None:
                    raise ValueError("invalid JPEG image")

                # Ultralytics inference is synchronous. Run it off the asyncio
                # event loop so health checks and other clients remain usable.
                print(f"[Server] Running YOLO detection on {img.shape}...")
                start = time.time()
                detections, _ = await detector.detect_async(img)
                process_ms = (time.time() - start) * 1000
                result = {
                    "count": len(detections),
                    "detections": detections,
                    "process_ms": round(process_ms, 2),
                }
                await websocket.send_text(json.dumps(result))
                print(f"[Server] Detection complete: {len(detections)} objects, {process_ms:.2f}ms")
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
    host = server_config.get("host", "0.0.0.0")
    port = server_config.get("port", 8090)
    print(f"Starting server on {host}:{port}")
    uvicorn.run(app, host=host, port=port)
