#!/usr/bin/env python3
"""
Batch process multiple images without reloading model
"""
import argparse
import cv2
import glob
from pathlib import Path
from ultralytics import YOLO
import time
import logging

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')
logger = logging.getLogger(__name__)


def batch_process_images(
    model_path: str,
    input_pattern: str,
    output_dir: str,
    confidence: float = 0.45,
    imgsz: int = 640,
    device: str = None
):
    """
    Process multiple images with single model load

    Args:
        model_path: Path to model weights
        input_pattern: Glob pattern for input images (e.g., "images/*.jpg")
        output_dir: Directory to save results
        confidence: Confidence threshold
        imgsz: Image size
        device: Device to use (cpu/cuda:0)
    """
    # Load model once
    logger.info(f"Loading model: {model_path}")
    start_load = time.time()
    model = YOLO(model_path)
    if device:
        model.to(device)
    load_time = (time.time() - start_load) * 1000
    logger.info(f"Model loaded in {load_time:.2f}ms")

    # Find images
    image_files = glob.glob(input_pattern)
    if not image_files:
        logger.error(f"No images found matching: {input_pattern}")
        return

    logger.info(f"Found {len(image_files)} images to process")

    # Create output directory
    output_path = Path(output_dir)
    output_path.mkdir(parents=True, exist_ok=True)

    # Process images
    total_time = 0
    total_detections = 0

    for i, image_file in enumerate(image_files, 1):
        logger.info(f"\n[{i}/{len(image_files)}] Processing: {image_file}")

        # Read image
        img = cv2.imread(image_file)
        if img is None:
            logger.warning(f"Failed to read: {image_file}")
            continue

        # Detect
        start_detect = time.time()
        results = model(img, conf=confidence, imgsz=imgsz, verbose=False)
        detect_time = (time.time() - start_detect) * 1000
        total_time += detect_time

        # Get results
        boxes = results[0].boxes
        num_detections = len(boxes)
        total_detections += num_detections

        logger.info(f"  Detected: {num_detections} persons")
        logger.info(f"  Time: {detect_time:.2f}ms")

        # Save result
        output_file = output_path / f"result_{Path(image_file).name}"
        results[0].plot()  # Annotate image
        cv2.imwrite(str(output_file), results[0].plot())
        logger.info(f"  Saved: {output_file}")

    # Summary
    avg_time = total_time / len(image_files) if image_files else 0
    logger.info("\n" + "="*50)
    logger.info("Batch Processing Summary:")
    logger.info(f"  Total images: {len(image_files)}")
    logger.info(f"  Total detections: {total_detections}")
    logger.info(f"  Total processing time: {total_time:.2f}ms")
    logger.info(f"  Average time per image: {avg_time:.2f}ms")
    logger.info(f"  Model load time: {load_time:.2f}ms")
    logger.info("="*50)


def main():
    parser = argparse.ArgumentParser(description="Batch process images")
    parser.add_argument("--model", type=str, default="model/yolo11s.pt", help="Model path")
    parser.add_argument("--input", type=str, required=True, help="Input pattern (e.g., 'images/*.jpg')")
    parser.add_argument("--output", type=str, default="results", help="Output directory")
    parser.add_argument("--conf", type=float, default=0.45, help="Confidence threshold")
    parser.add_argument("--imgsz", type=int, default=640, help="Image size")
    parser.add_argument("--device", type=str, default=None, help="Device (cpu/cuda:0)")

    args = parser.parse_args()

    batch_process_images(
        model_path=args.model,
        input_pattern=args.input,
        output_dir=args.output,
        confidence=args.conf,
        imgsz=args.imgsz,
        device=args.device
    )


if __name__ == "__main__":
    main()
