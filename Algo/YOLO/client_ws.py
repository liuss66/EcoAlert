"""
WebSocket client - process video stream
"""
import cv2
import numpy as np
import json
import asyncio
import websockets
import time
import argparse
import logging
from pathlib import Path

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')
logger = logging.getLogger(__name__)


class YOLOWebSocketClient:
    def __init__(self, server_url="ws://localhost:8090"):
        self.server_url = server_url

    async def detect_frame(self, ws, frame, max_width=1280, quality=60, confidence=0.45):
        # Resize
        h, w = frame.shape[:2]
        if w > max_width:
            scale = max_width / w
            frame = cv2.resize(frame, (max_width, int(h * scale)))

        # Encode
        _, buffer = cv2.imencode('.jpg', frame, [cv2.IMWRITE_JPEG_QUALITY, quality])
        img_bytes = buffer.tobytes()

        # Send and receive
        start = time.time()
        await ws.send(json.dumps({"type": "options", "confidence": confidence}))
        await ws.send(img_bytes)
        result = await ws.recv()
        elapsed = (time.time() - start) * 1000

        return json.loads(result), elapsed


async def process_video(args):
    client = YOLOWebSocketClient(args.server)

    logger.info(f"Connecting to {args.server}/ws...")
    async with websockets.connect(f"{args.server}/ws") as ws:
        logger.info("Connected!")

        # Open video
        cap = cv2.VideoCapture(args.source)
        fps = cap.get(cv2.CAP_PROP_FPS)
        total_frames = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
        width = int(cap.get(cv2.CAP_PROP_FRAME_WIDTH))
        height = int(cap.get(cv2.CAP_PROP_FRAME_HEIGHT))
        logger.info(f"Video: {width}x{height} @ {fps:.2f}fps, {total_frames} frames")

        # Output video
        video_writer = None
        if args.output:
            fourcc = cv2.VideoWriter_fourcc(*'mp4v')
            video_writer = cv2.VideoWriter(args.output, fourcc, fps, (width, height))

        frame_count = 0
        processed_count = 0
        total_detections = 0
        times = []
        start_time = time.time()

        while True:
            ret, frame = cap.read()
            if not ret:
                break

            if frame_count % args.frame_interval != 0:
                frame_count += 1
                continue

            # Detect
            result, elapsed = await client.detect_frame(
                ws, frame, args.max_width, args.quality, args.confidence
            )
            times.append(elapsed)
            count = result.get('count', 0)
            total_detections += count

            # Visualize
            annotated = frame.copy()
            for det in result.get('detections', []):
                x, y, bw, bh = det['bbox']
                x1 = int((x - bw/2) * width)
                y1 = int((y - bh/2) * height)
                x2 = int((x + bw/2) * width)
                y2 = int((y + bh/2) * height)
                cv2.rectangle(annotated, (x1, y1), (x2, y2), (0, 255, 0), 2)
                cv2.putText(annotated, f"{det['confidence']:.2f}", (x1, y1-5),
                           cv2.FONT_HERSHEY_SIMPLEX, 0.5, (0, 255, 0), 1)

            # FPS overlay
            if args.display:
                avg_time = np.mean(times[-20:])
                fps_real = 1000 / avg_time if avg_time > 0 else 0
                cv2.putText(annotated, f"FPS: {fps_real:.1f}", (10, 30),
                           cv2.FONT_HERSHEY_SIMPLEX, 1, (0, 255, 255), 2)

            if video_writer:
                video_writer.write(annotated)

            if args.display:
                cv2.imshow("YOLO WebSocket", annotated)
                if cv2.waitKey(1) & 0xFF == ord('q'):
                    break

            processed_count += 1
            if processed_count % 10 == 0:
                progress = (frame_count + 1) / total_frames * 100
                logger.info(f"Progress: {progress:.1f}% | Persons: {count} | Time: {elapsed:.2f}ms")

            frame_count += 1

        cap.release()
        if video_writer:
            video_writer.release()
        if args.display:
            cv2.destroyAllWindows()

        # Summary
        if times:
            avg = np.mean(times)
            logger.info(f"\nAverage time: {avg:.2f}ms/frame")
            logger.info(f"Max FPS: {1000/avg:.2f}")
            logger.info(f"Total detections: {total_detections}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True)
    parser.add_argument("--server", default="ws://localhost:8090")
    parser.add_argument("--output", default=None)
    parser.add_argument("--frame-interval", type=int, default=1)
    parser.add_argument("--max-width", type=int, default=1280)
    parser.add_argument("--quality", type=int, default=60)
    parser.add_argument("--confidence", type=float, default=0.45)
    parser.add_argument("--display", action="store_true")
    args = parser.parse_args()

    asyncio.run(process_video(args))
