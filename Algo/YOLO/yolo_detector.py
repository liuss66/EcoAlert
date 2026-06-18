"""
YOLO Detector wrapper for the server
"""
import numpy as np
import time
from typing import List, Optional, Tuple
from ultralytics import YOLO
import base64
from PIL import Image
import io
import asyncio
from concurrent.futures import ThreadPoolExecutor


class YOLODetector:
    """YOLO detector class for handling model inference"""

    def __init__(
        self,
        model_name: str = "model/yolo11s.pt",
        confidence: float = 0.45,
        iou_threshold: float = 0.35,
        classes: List[int] = [0],
        device: str = "auto",
        imgsz: int = 640,
        max_workers: int = 1
    ):
        """
        Initialize YOLO detector

        Args:
            model_name: Path to model weights
            confidence: Confidence threshold
            iou_threshold: NMS IoU threshold
            classes: List of class IDs to detect
            device: Device to use (auto, cpu, cuda:0)
            imgsz: Inference image size
            max_workers: Inference workers. Keep this at 1 because one
                Ultralytics model instance is not safe for concurrent inference.
        """
        self.model_name = model_name
        self.confidence = confidence
        self.iou_threshold = iou_threshold
        self.classes = classes
        self.imgsz = imgsz
        self.max_workers = max_workers

        # Auto-detect device
        if device == "auto":
            try:
                import torch
                self.device = "cuda:0" if torch.cuda.is_available() else "cpu"
            except ImportError:
                self.device = "cpu"
        else:
            self.device = device

        # A checked-in config must also work on machines without CUDA.  Falling
        # back here gives a usable server instead of failing before port 8090 is
        # opened.
        if self.device.startswith("cuda"):
            try:
                import torch
                if not torch.cuda.is_available():
                    print(f"CUDA device {self.device} is unavailable; falling back to CPU")
                    self.device = "cpu"
            except ImportError:
                self.device = "cpu"

        # Load model
        print(f"Loading model: {model_name}")
        self.model = YOLO(model_name)
        if self.device != "auto":
            self.model.to(self.device)
        print(f"Model loaded on device: {self.device}")

        # Thread pool for async detection
        self.executor = ThreadPoolExecutor(max_workers=max_workers)

    def decode_image(self, image_base64: str) -> np.ndarray:
        """
        Decode base64 image to numpy array

        Args:
            image_base64: Base64 encoded image string

        Returns:
            Numpy array (RGB)
        """
        # Remove data URI prefix if present
        if "," in image_base64:
            image_base64 = image_base64.split(",")[1]

        # Decode base64
        image_bytes = base64.b64decode(image_base64)

        # Load as PIL Image
        image = Image.open(io.BytesIO(image_bytes))

        # Convert to RGB if necessary
        if image.mode != "RGB":
            image = image.convert("RGB")

        # Convert to numpy array
        return np.array(image)

    def detect(
        self,
        image: np.ndarray,
        confidence: Optional[float] = None,
        classes: Optional[List[int]] = None
    ) -> Tuple[List[dict], float]:
        """
        Detect objects in image

        Args:
            image: Numpy array (RGB)
            confidence: Override confidence threshold
            classes: Override class filter

        Returns:
            Tuple of (detections list, processing time in ms)
        """
        start_time = time.time()

        # Use override parameters or defaults
        conf = confidence if confidence is not None else self.confidence
        cls = classes if classes is not None else self.classes

        # Run inference
        results = self.model(
            image,
            conf=conf,
            iou=self.iou_threshold,
            imgsz=self.imgsz,
            classes=cls,
            verbose=False
        )

        # Process results
        detections = []
        if len(results) > 0 and results[0].boxes is not None:
            boxes = results[0].boxes
            img_height, img_width = image.shape[:2]

            for i in range(len(boxes)):
                # Get box coordinates (xyxy format)
                x1, y1, x2, y2 = boxes.xyxy[i].cpu().numpy()

                # Convert to xywh normalized
                x = float((x1 + x2) / 2 / img_width)
                y = float((y1 + y2) / 2 / img_height)
                w = float((x2 - x1) / img_width)
                h = float((y2 - y1) / img_height)

                # Get class and confidence
                class_id = int(boxes.cls[i].cpu().numpy())
                class_name = self.model.names[class_id]
                conf_score = float(boxes.conf[i].cpu().numpy())

                detections.append({
                    "class_id": class_id,
                    "class_name": class_name,
                    "confidence": round(conf_score, 4),
                    "bbox": [round(x, 4), round(y, 4), round(w, 4), round(h, 4)]
                })

        process_ms = (time.time() - start_time) * 1000
        return detections, process_ms

    async def detect_async(
        self,
        image: np.ndarray,
        confidence: Optional[float] = None,
        classes: Optional[List[int]] = None
    ) -> Tuple[List[dict], float]:
        """
        Async version of detect using thread pool

        Args:
            image: Numpy array (RGB)
            confidence: Override confidence threshold
            classes: Override class filter

        Returns:
            Tuple of (detections list, processing time in ms)
        """
        loop = asyncio.get_running_loop()
        result = await loop.run_in_executor(
            self.executor,
            lambda: self.detect(image, confidence, classes)
        )
        return result
