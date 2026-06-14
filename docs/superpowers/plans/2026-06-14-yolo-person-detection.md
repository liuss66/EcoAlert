# YOLO Person Detection Sidecar Service Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Python FastAPI sidecar service that accepts single images and returns YOLOv8 person detection results in JSON format, handling occluded and back-view subjects.

**Architecture:** Standalone Python service in `Algo/YOLO/` with FastAPI REST API. YOLOv8 model loaded at startup, single-frame inference on each request. Returns normalized bounding boxes, confidence scores, and class labels.

**Tech Stack:** Python 3.10+, FastAPI, ultralytics (YOLOv8), uvicorn, Pydantic

---

## File Structure

```
Algo/YOLO/
├── server.py          # FastAPI application, routes, startup logic
├── yolo_detector.py   # YOLO model wrapper, detection logic
├── models.py          # Pydantic request/response schemas
├── config.yaml        # Service configuration
├── requirements.txt   # Python dependencies
└── tests/
    ├── __init__.py
    ├── test_detector.py    # Unit tests for YOLO detector
    └── test_server.py      # Integration tests for API endpoints
```

---

### Task 1: Project Setup and Dependencies

**Files:**
- Create: `Algo/YOLO/requirements.txt`
- Create: `Algo/YOLO/config.yaml`

- [ ] **Step 1: Create requirements.txt**

```
# Algo/YOLO/requirements.txt
fastapi>=0.104.0
uvicorn[standard]>=0.24.0
ultralytics>=8.0.200
pydantic>=2.0.0
pyyaml>=6.0
python-multipart>=0.0.6
```

- [ ] **Step 2: Create config.yaml**

```yaml
# Algo/YOLO/config.yaml
server:
  host: "0.0.0.0"
  port: 8090

model:
  name: "yolov8m.pt"
  confidence: 0.5
  iou_threshold: 0.45
  classes: [0]  # COCO class 0 = person
  device: "auto"  # auto, cpu, cuda:0
  imgsz: 640
```

- [ ] **Step 3: Verify dependencies install**

Run: `pip install -r Algo/YOLO/requirements.txt`
Expected: All packages install successfully

---

### Task 2: Pydantic Models

**Files:**
- Create: `Algo/YOLO/models.py`

- [ ] **Step 1: Create request/response models**

```python
# Algo/YOLO/models.py
from pydantic import BaseModel, Field
from typing import List, Optional


class DetectionRequest(BaseModel):
    image: str = Field(..., description="Base64 encoded image (JPEG/PNG)")
    confidence: Optional[float] = Field(None, ge=0.0, le=1.0, description="Override confidence threshold")
    classes: Optional[List[int]] = Field(None, description="Override class filter (default: [0] for person)")


class Detection(BaseModel):
    class_id: int = Field(..., description="COCO class ID")
    class_name: str = Field(..., description="Class name")
    confidence: float = Field(..., ge=0.0, le=1.0)
    bbox: List[float] = Field(..., description="[x, y, w, h] normalized 0-1")


class DetectionResponse(BaseModel):
    success: bool
    detections: List[Detection]
    count: int
    process_ms: float


class ErrorResponse(BaseModel):
    success: bool = False
    error: str
```

- [ ] **Step 2: Verify models import correctly**

Run: `python -c "from models import DetectionRequest, DetectionResponse; print('OK')"`
Expected: Prints "OK"

---

### Task 3: YOLO Detector Module

**Files:**
- Create: `Algo/YOLO/yolo_detector.py`

- [ ] **Step 1: Create YOLODetector class**

```python
# Algo/YOLO/yolo_detector.py
import base64
import io
import time
from pathlib import Path
from typing import List, Optional, Tuple

import numpy as np
from PIL import Image
from ultralytics import YOLO


COCO_CLASSES = {
    0: "person",
    1: "bicycle",
    2: "car",
    # ... full COCO classes omitted, only person is used
}


class YOLODetector:
    def __init__(
        self,
        model_name: str = "yolov8m.pt",
        confidence: float = 0.5,
        iou_threshold: float = 0.45,
        classes: Optional[List[int]] = None,
        device: str = "auto",
        imgsz: int = 640,
    ):
        self.confidence = confidence
        self.iou_threshold = iou_threshold
        self.classes = classes if classes is not None else [0]
        self.imgsz = imgsz

        if device == "auto":
            import torch
            device = "cuda:0" if torch.cuda.is_available() else "cpu"

        self.model = YOLO(model_name)
        self.device = device

    def detect_from_base64(
        self,
        image_b64: str,
        confidence: Optional[float] = None,
        classes: Optional[List[int]] = None,
    ) -> Tuple[List[dict], float]:
        conf = confidence if confidence is not None else self.confidence
        cls = classes if classes is not None else self.classes

        img_bytes = base64.b64decode(image_b64)
        image = Image.open(io.BytesIO(img_bytes)).convert("RGB")
        image_np = np.array(image)

        start = time.perf_counter()
        results = self.model.predict(
            source=image_np,
            conf=conf,
            iou=self.iou_threshold,
            classes=cls,
            imgsz=self.imgsz,
            device=self.device,
            verbose=False,
        )
        process_ms = (time.perf_counter() - start) * 1000

        detections = []
        for r in results:
            boxes = r.boxes
            if boxes is None:
                continue
            for box in boxes:
                xyxy = box.xyxy[0].cpu().numpy()
                conf_val = float(box.conf[0])
                cls_id = int(box.cls[0])

                img_h, img_w = image_np.shape[:2]
                x1, y1, x2, y2 = xyxy
                bbox = [
                    round(float(x1) / img_w, 4),
                    round(float(y1) / img_h, 4),
                    round(float((x2 - x1)) / img_w, 4),
                    round(float((y2 - y1)) / img_h, 4),
                ]

                detections.append({
                    "class_id": cls_id,
                    "class_name": COCO_CLASSES.get(cls_id, f"class_{cls_id}"),
                    "confidence": round(conf_val, 4),
                    "bbox": bbox,
                })

        return detections, process_ms
```

- [ ] **Step 2: Verify detector loads model**

Run: `python -c "from yolo_detector import YOLODetector; d = YOLODetector(); print('Model loaded')"`
Expected: Prints "Model loaded" (downloads model on first run)

---

### Task 4: FastAPI Server

**Files:**
- Create: `Algo/YOLO/server.py`

- [ ] **Step 1: Create FastAPI application**

```python
# Algo/YOLO/server.py
import yaml
from pathlib import Path
from contextlib import asynccontextmanager

from fastapi import FastAPI, HTTPException
from fastapi.responses import JSONResponse

from models import DetectionRequest, DetectionResponse, ErrorResponse, Detection
from yolo_detector import YOLODetector


config_path = Path(__file__).parent / "config.yaml"
with open(config_path) as f:
    config = yaml.safe_load(f)

detector: YOLODetector = None


@asynccontextmanager
async def lifespan(app: FastAPI):
    global detector
    model_cfg = config["model"]
    detector = YOLODetector(
        model_name=model_cfg["name"],
        confidence=model_cfg["confidence"],
        iou_threshold=model_cfg["iou_threshold"],
        classes=model_cfg["classes"],
        device=model_cfg["device"],
        imgsz=model_cfg["imgsz"],
    )
    yield


app = FastAPI(
    title="YOLO Person Detection",
    version="1.0.0",
    lifespan=lifespan,
)


@app.post("/detect", response_model=DetectionResponse)
async def detect(request: DetectionRequest):
    try:
        detections_raw, process_ms = detector.detect_from_base64(
            image_b64=request.image,
            confidence=request.confidence,
            classes=request.classes,
        )
        detections = [Detection(**d) for d in detections_raw]
        return DetectionResponse(
            success=True,
            detections=detections,
            count=len(detections),
            process_ms=round(process_ms, 2),
        )
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


@app.get("/health")
async def health():
    return {"status": "ok", "model": config["model"]["name"]}


if __name__ == "__main__":
    import uvicorn
    server_cfg = config["server"]
    uvicorn.run(
        "server:app",
        host=server_cfg["host"],
        port=server_cfg["port"],
        reload=False,
    )
```

- [ ] **Step 2: Verify server starts**

Run: `cd Algo/YOLO && python server.py`
Expected: Server starts on port 8090, logs show "Uvicorn running on http://0.0.0.0:8090"

- [ ] **Step 3: Test health endpoint**

Run: `curl http://localhost:8090/health`
Expected: `{"status":"ok","model":"yolov8m.pt"}`

---

### Task 5: Unit Tests for Detector

**Files:**
- Create: `Algo/YOLO/tests/__init__.py`
- Create: `Algo/YOLO/tests/test_detector.py`

- [ ] **Step 1: Create test_detector.py with synthetic image test**

```python
# Algo/YOLO/tests/__init__.py
# empty

# Algo/YOLO/tests/test_detector.py
import base64
import io
import sys
from pathlib import Path

import numpy as np
import pytest
from PIL import Image

sys.path.insert(0, str(Path(__file__).parent.parent))

from yolo_detector import YOLODetector


@pytest.fixture(scope="module")
def detector():
    return YOLODetector(model_name="yolov8n.pt", device="cpu")


def create_test_image_b64(width: int = 640, height: int = 480, color=(128, 128, 128)) -> str:
    img = Image.new("RGB", (width, height), color)
    buf = io.BytesIO()
    img.save(buf, format="JPEG")
    return base64.b64encode(buf.getvalue()).decode()


def test_detector_returns_list(detector):
    b64 = create_test_image_b64()
    detections, process_ms = detector.detect_from_base64(b64)
    assert isinstance(detections, list)
    assert process_ms > 0


def test_detector_respects_confidence_override(detector):
    b64 = create_test_image_b64()
    detections_low, _ = detector.detect_from_base64(b64, confidence=0.01)
    detections_high, _ = detector.detect_from_base64(b64, confidence=0.99)
    assert len(detections_high) <= len(detections_low)


def test_detector_bbox_normalized(detector):
    b64 = create_test_image_b64(640, 480)
    detections, _ = detector.detect_from_base64(b64, confidence=0.01)
    for det in detections:
        bbox = det["bbox"]
        assert len(bbox) == 4
        assert all(0.0 <= v <= 1.0 for v in bbox), f"Bbox out of range: {bbox}"
```

- [ ] **Step 2: Run tests**

Run: `cd Algo/YOLO && python -m pytest tests/test_detector.py -v`
Expected: All 3 tests PASS

---

### Task 6: Integration Test for API

**Files:**
- Create: `Algo/YOLO/tests/test_server.py`

- [ ] **Step 1: Create test_server.py**

```python
# Algo/YOLO/tests/test_server.py
import base64
import io
import sys
from pathlib import Path

import pytest
from fastapi.testclient import TestClient
from PIL import Image

sys.path.insert(0, str(Path(__file__).parent.parent))

from server import app


@pytest.fixture(scope="module")
def client():
    return TestClient(app)


def create_test_image_b64(width=640, height=480, color=(128, 128, 128)) -> str:
    img = Image.new("RGB", (width, height), color)
    buf = io.BytesIO()
    img.save(buf, format="JPEG")
    return base64.b64encode(buf.getvalue()).decode()


def test_health_endpoint(client):
    resp = client.get("/health")
    assert resp.status_code == 200
    data = resp.json()
    assert data["status"] == "ok"


def test_detect_returns_success(client):
    b64 = create_test_image_b64()
    resp = client.post("/detect", json={"image": b64})
    assert resp.status_code == 200
    data = resp.json()
    assert data["success"] is True
    assert isinstance(data["detections"], list)
    assert data["process_ms"] > 0


def test_detect_with_high_confidence(client):
    b64 = create_test_image_b64()
    resp = client.post("/detect", json={"image": b64, "confidence": 0.99})
    assert resp.status_code == 200
    data = resp.json()
    assert data["success"] is True


def test_detect_invalid_image(client):
    resp = client.post("/detect", json={"image": "not-valid-base64"})
    assert resp.status_code == 500
```

- [ ] **Step 2: Run integration tests**

Run: `cd Algo/YOLO && python -m pytest tests/test_server.py -v`
Expected: All 4 tests PASS

---

### Task 7: Manual Verification with Real Image

- [ ] **Step 1: Start the server**

Run: `cd Algo/YOLO && python server.py`

- [ ] **Step 2: Test with a real person image**

Prepare a test image with a person, encode to base64, and call the API:

```bash
python -c "
import base64, requests, sys
with open(sys.argv[1], 'rb') as f:
    b64 = base64.b64encode(f.read()).decode()
resp = requests.post('http://localhost:8090/detect', json={'image': b64})
print(resp.json())
" path/to/person_image.jpg
```

Expected: Returns detections with `class_name: "person"` and reasonable confidence

---

## Commit Strategy

After each task completes successfully:

```bash
git add Algo/YOLO/
git commit -m "feat(yolo): add YOLO person detection sidecar service

- FastAPI server with /detect and /health endpoints
- YOLOv8 detector with base64 image input
- Normalized bounding box output
- Configurable confidence and class filters
- Unit and integration tests"
```
