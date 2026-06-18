#!/usr/bin/env python3
"""
YOLOv8n Person Detection for Overhead Camera Views
Detects persons in single frames, optimized for scenarios with occlusions
from office furniture (desks, chairs).
Supports both image and video inputs (each frame processed independently).
"""

import argparse
import cv2
import numpy as np
from pathlib import Path
from ultralytics import YOLO
import logging
import time
from typing import Tuple, Optional

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)


def load_model(model_path: str = 'model/yolov8n.pt', device: str = None):
    """
    Load YOLOv8n model.

    Args:
        model_path: Path to model weights (default: yolov8n.pt)
        device: Device to use ('cpu', 'cuda:0', etc.)

    Returns:
        Loaded YOLO model
    """
    logger.info(f"Loading model: {model_path}")
    model = YOLO(model_path)

    if device:
        model.to(device)

    return model


def detect_persons(
    model,
    image_path: str,
    conf_threshold: float = 0.25,
    imgsz: int = 640,
    iou_threshold: float = 0.45
):
    """
    Detect persons in a single image.

    Args:
        model: Loaded YOLO model
        image_path: Path to input image
        conf_threshold: Confidence threshold (lower for occluded scenarios)
        imgsz: Inference image size
        iou_threshold: NMS IoU threshold

    Returns:
        Detection results and original image
    """
    logger.info(f"Processing image: {image_path}")
    logger.info(f"Confidence threshold: {conf_threshold}, IoU threshold: {iou_threshold}")

    # Read image
    image = cv2.imread(image_path)
    if image is None:
        raise ValueError(f"Failed to read image: {image_path}")

    # Run inference
    # Filter for person class (class 0 in COCO dataset)
    results = model(
        image,
        conf=conf_threshold,
        iou=iou_threshold,
        imgsz=imgsz,
        classes=[0],  # Only detect 'person' class
        verbose=True
    )

    return results, image


def visualize_detections(
    image,
    results,
    save_path: str = None,
    show_conf: bool = True,
    line_thickness: int = 2
):
    """
    Visualize detection results on image.

    Args:
        image: Original image
        results: YOLO detection results
        save_path: Path to save annotated image
        show_conf: Whether to show confidence scores
        line_thickness: Bounding box line thickness

    Returns:
        Annotated image
    """
    annotated_image = image.copy()

    # Extract detections
    detections = results[0].boxes

    if len(detections) == 0:
        logger.warning("No persons detected in the image")
        return annotated_image

    logger.info(f"Detected {len(detections)} person(s)")

    # Draw bounding boxes
    for i, det in enumerate(detections):
        # Get bounding box coordinates
        x1, y1, x2, y2 = det.xyxy[0].cpu().numpy().astype(int)

        # Get confidence score
        conf = det.conf[0].cpu().item()

        # Get class ID (should be 0 for person)
        cls = int(det.cls[0].cpu().item())

        # Color for bounding box (green for person)
        color = (0, 255, 0)

        # Draw rectangle
        cv2.rectangle(annotated_image, (x1, y1), (x2, y2), color, line_thickness)

        # Prepare label
        if show_conf:
            label = f'Person {i+1}: {conf:.2f}'
        else:
            label = f'Person {i+1}'

        # Calculate text position
        (text_width, text_height), baseline = cv2.getTextSize(
            label,
            cv2.FONT_HERSHEY_SIMPLEX,
            0.6,
            2
        )

        # Draw text background
        cv2.rectangle(
            annotated_image,
            (x1, y1 - text_height - 10),
            (x1 + text_width, y1),
            color,
            -1
        )

        # Draw text
        cv2.putText(
            annotated_image,
            label,
            (x1, y1 - 5),
            cv2.FONT_HERSHEY_SIMPLEX,
            0.6,
            (255, 255, 255),
            2
        )

    # Save annotated image if path provided
    if save_path:
        cv2.imwrite(save_path, annotated_image)
        logger.info(f"Annotated image saved to: {save_path}")

    return annotated_image


def save_crops(image, results, output_dir: str, min_size: int = 50):
    """
    Save cropped person detections.

    Args:
        image: Original image
        results: YOLO detection results
        output_dir: Directory to save cropped images
        min_size: Minimum crop size to filter out tiny detections
    """
    output_path = Path(output_dir)
    output_path.mkdir(parents=True, exist_ok=True)

    detections = results[0].boxes

    if len(detections) == 0:
        logger.info("No detections to crop")
        return

    cropped_count = 0
    for i, det in enumerate(detections):
        # Get bounding box coordinates
        x1, y1, x2, y2 = det.xyxy[0].cpu().numpy().astype(int)

        # Filter out very small detections
        width = x2 - x1
        height = y2 - y1
        if width < min_size or height < min_size:
            logger.debug(f"Skipping small detection {i+1}: {width}x{height}")
            continue

        # Crop person from image
        crop = image[y1:y2, x1:x2]

        # Save crop
        crop_path = output_path / f"person_{i+1:03d}.jpg"
        cv2.imwrite(str(crop_path), crop)
        cropped_count += 1

    logger.info(f"Saved {cropped_count} cropped person images to: {output_dir}")


def get_detection_summary(results):
    """
    Generate a summary of detection results.

    Args:
        results: YOLO detection results

    Returns:
        Dictionary with detection statistics
    """
    detections = results[0].boxes

    if len(detections) == 0:
        return {
            'count': 0,
            'confidences': [],
            'avg_confidence': 0.0,
            'boxes': []
        }

    confidences = detections.conf.cpu().numpy().tolist()
    boxes = detections.xyxy.cpu().numpy().tolist()

    summary = {
        'count': len(detections),
        'confidences': confidences,
        'avg_confidence': np.mean(confidences),
        'boxes': boxes
    }

    return summary


def is_video_file(file_path: str) -> bool:
    """
    Check if a file is a video based on extension.

    Args:
        file_path: Path to file

    Returns:
        True if video file, False otherwise
    """
    video_extensions = {'.mp4', '.avi', '.mov', '.mkv', '.flv', '.wmv', '.webm', '.m4v'}
    return Path(file_path).suffix.lower() in video_extensions


def process_video(
    model,
    video_path: str,
    conf_threshold: float = 0.25,
    imgsz: int = 640,
    iou_threshold: float = 0.45,
    frame_interval: int = 1,
    output_dir: str = 'output/',
    save_video: bool = False,
    save_frames: bool = False,
    max_frames: Optional[int] = None,
    show_conf: bool = True,
    line_thickness: int = 2,
    display: bool = False,
    display_scale: float = 1.0,
    original_speed: bool = True
):
    """
    Process video file, detecting persons in each frame independently.

    Args:
        model: Loaded YOLO model
        video_path: Path to video file
        conf_threshold: Confidence threshold
        imgsz: Inference image size
        iou_threshold: NMS IoU threshold
        frame_interval: Process every N frames (1 = all frames)
        output_dir: Output directory
        save_video: Whether to save output video
        save_frames: Whether to save individual frames
        max_frames: Maximum frames to process (None = all)
        show_conf: Whether to show confidence scores
        line_thickness: Bounding box line thickness
        display: Whether to display video in real-time
        display_scale: Display window scale (0.5 = half size)
        original_speed: Play at original video speed (if display=True)

    Returns:
        Dictionary with processing statistics
    """
    logger.info(f"Processing video: {video_path}")
    logger.info(f"Frame interval: {frame_interval}")
    if display:
        logger.info("Real-time display enabled")
        logger.info("Controls: 'q' to quit, 'space' to pause/resume")

    # Open video
    cap = cv2.VideoCapture(video_path)
    if not cap.isOpened():
        raise ValueError(f"Failed to open video: {video_path}")

    # Get video properties
    fps = cap.get(cv2.CAP_PROP_FPS)
    total_frames = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
    width = int(cap.get(cv2.CAP_PROP_FRAME_WIDTH))
    height = int(cap.get(cv2.CAP_PROP_FRAME_HEIGHT))

    logger.info(f"Video properties:")
    logger.info(f"  Resolution: {width}x{height}")
    logger.info(f"  FPS: {fps:.2f}")
    logger.info(f"  Total frames: {total_frames}")
    logger.info(f"  Duration: {total_frames/fps:.2f}s")

    # Setup output video writer if needed
    video_writer = None
    if save_video:
        output_video_path = Path(output_dir) / f"{Path(video_path).stem}_detected.mp4"
        fourcc = cv2.VideoWriter_fourcc(*'mp4v')
        out_fps = fps / frame_interval  # Adjust FPS for frame skipping
        video_writer = cv2.VideoWriter(
            str(output_video_path),
            fourcc,
            out_fps,
            (width, height)
        )
        logger.info(f"Output video: {output_video_path}")

    # Setup frame output directory if needed
    if save_frames:
        frames_dir = Path(output_dir) / f"{Path(video_path).stem}_frames"
        frames_dir.mkdir(parents=True, exist_ok=True)
        logger.info(f"Output frames directory: {frames_dir}")

    # Process frames
    frame_count = 0
    processed_count = 0
    total_detections = 0
    start_time = time.time()
    paused = False

    # Calculate delay for original speed playback
    frame_delay = int(1000 / fps) if original_speed and fps > 0 else 1

    try:
        while True:
            ret, frame = cap.read()
            if not ret:
                break

            # Skip frames based on interval
            if frame_count % frame_interval != 0:
                frame_count += 1
                continue

            # Check if we've reached max frames
            if max_frames and processed_count >= max_frames:
                logger.info(f"Reached max frames limit: {max_frames}")
                break

            # Process frame (as independent image)
            results = model(
                frame,
                conf=conf_threshold,
                iou=iou_threshold,
                imgsz=imgsz,
                classes=[0],  # Only 'person' class
                verbose=False
            )

            # Visualize detections
            annotated_frame = visualize_detections(
                frame,
                results,
                save_path=None,  # Don't save individual file here
                show_conf=show_conf,
                line_thickness=line_thickness
            )

            # Count detections
            num_detections = len(results[0].boxes)
            total_detections += num_detections

            # Calculate and display FPS
            processed_count += 1
            elapsed = time.time() - start_time
            fps_process = processed_count / elapsed if elapsed > 0 else 0
            progress = (frame_count + 1) / total_frames * 100

            # Add FPS and info overlay to the frame
            if display:
                # Create info text
                info_text = f"Frame {frame_count+1}/{total_frames} ({progress:.1f}%)"
                fps_text = f"Processing: {fps_process:.1f} FPS"
                detect_text = f"Persons: {num_detections}"

                # Add text to frame
                cv2.putText(annotated_frame, info_text, (10, 30),
                           cv2.FONT_HERSHEY_SIMPLEX, 0.7, (0, 255, 255), 2)
                cv2.putText(annotated_frame, fps_text, (10, 60),
                           cv2.FONT_HERSHEY_SIMPLEX, 0.7, (0, 255, 255), 2)
                cv2.putText(annotated_frame, detect_text, (10, 90),
                           cv2.FONT_HERSHEY_SIMPLEX, 0.7, (0, 255, 255), 2)

                if paused:
                    cv2.putText(annotated_frame, "PAUSED", (10, 120),
                               cv2.FONT_HERSHEY_SIMPLEX, 1.0, (0, 0, 255), 3)

                # Display frame
                display_frame = annotated_frame
                if display_scale != 1.0:
                    new_width = int(width * display_scale)
                    new_height = int(height * display_scale)
                    display_frame = cv2.resize(annotated_frame, (new_width, new_height))

                cv2.imshow(f"YOLOv8n Detection - {Path(video_path).name}", display_frame)

                # Handle keyboard input
                key = cv2.waitKey(frame_delay if not paused else 0) & 0xFF
                if key == ord('q'):
                    logger.info("Quit signal received, stopping...")
                    break
                elif key == ord(' '):
                    paused = not paused
                    if paused:
                        logger.info("Paused. Press SPACE to resume.")
                    else:
                        logger.info("Resumed.")
            else:
                # No display, just log progress
                if processed_count % 10 == 0:  # Update every 10 frames
                    logger.info(
                        f"Progress: {progress:.1f}% | "
                        f"Frame {frame_count+1}/{total_frames} | "
                        f"Persons: {num_detections} | "
                        f"Processing FPS: {fps_process:.1f}"
                    )

            # Save frame if requested
            if save_frames:
                frame_path = frames_dir / f"frame_{frame_count:06d}.jpg"
                cv2.imwrite(str(frame_path), annotated_frame)

            # Write to output video if enabled
            if video_writer:
                video_writer.write(annotated_frame)

            frame_count += 1

    finally:
        cap.release()
        if video_writer:
            video_writer.release()
        if display:
            cv2.destroyAllWindows()

    # Calculate statistics
    elapsed_time = time.time() - start_time
    avg_detections = total_detections / processed_count if processed_count > 0 else 0

    stats = {
        'total_frames': total_frames,
        'processed_frames': processed_count,
        'frame_interval': frame_interval,
        'total_detections': total_detections,
        'avg_detections_per_frame': avg_detections,
        'processing_time': elapsed_time,
        'processing_fps': processed_count / elapsed_time if elapsed_time > 0 else 0,
        'video_fps': fps,
        'video_duration': total_frames / fps if fps > 0 else 0
    }

    logger.info("\n" + "="*50)
    logger.info("Video Processing Summary:")
    logger.info(f"  Total frames in video: {stats['total_frames']}")
    logger.info(f"  Frames processed: {stats['processed_frames']}")
    logger.info(f"  Frame interval: {stats['frame_interval']}")
    logger.info(f"  Total person detections: {stats['total_detections']}")
    logger.info(f"  Avg detections per frame: {stats['avg_detections_per_frame']:.2f}")
    logger.info(f"  Processing time: {stats['processing_time']:.2f}s")
    logger.info(f"  Processing speed: {stats['processing_fps']:.1f} FPS")
    logger.info(f"  Video duration: {stats['video_duration']:.2f}s")
    if save_video:
        logger.info(f"  Output video: {output_video_path}")
    if save_frames:
        logger.info(f"  Output frames: {frames_dir}")
    logger.info("="*50)

    return stats


def main():
    parser = argparse.ArgumentParser(
        description='YOLOv8n Person Detection for Overhead Camera Views',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Basic image detection
  python detect.py --source image.jpg

  # Image with lower confidence for occlusions
  python detect.py --source image.jpg --conf 0.15

  # Video detection (process each frame independently)
  python detect.py --source video.mp4

  # Video with real-time display
  python detect.py --source video.mp4 --display

  # Video display at half size, process as fast as possible
  python detect.py --source video.mp4 --display --display-scale 0.5 --no-original-speed

  # Video with frame skipping (process every 5th frame)
  python detect.py --source video.mp4 --frame-interval 5

  # Video and save output video
  python detect.py --source video.mp4 --save-video

  # Video display and save at the same time
  python detect.py --source video.mp4 --display --save-video

  # Video and save individual frames
  python detect.py --source video.mp4 --save-frames

  # Process first 100 frames only
  python detect.py --source video.mp4 --max-frames 100

  # Use GPU
  python detect.py --source video.mp4 --device cuda:0
        """
    )

    # Required arguments
    parser.add_argument(
        '--source',
        type=str,
        required=True,
        help='Path to input image or video file'
    )

    # Model arguments
    parser.add_argument(
        '--model',
        type=str,
        default='model/yolov8n.pt',
        help='Path to model weights (default: model/yolov8n.pt)'
    )

    parser.add_argument(
        '--device',
        type=str,
        default=None,
        help='Device to use (cpu, cuda:0, etc.)'
    )

    # Detection arguments
    parser.add_argument(
        '--conf',
        type=float,
        default=0.25,
        help='Confidence threshold (default: 0.25, lower for occluded scenes)'
    )

    parser.add_argument(
        '--iou',
        type=float,
        default=0.45,
        help='NMS IoU threshold (default: 0.45)'
    )

    parser.add_argument(
        '--imgsz',
        type=int,
        default=640,
        help='Inference image size (default: 640)'
    )

    # Video-specific arguments
    parser.add_argument(
        '--frame-interval',
        type=int,
        default=1,
        help='Process every N frames for video (default: 1, process all frames)'
    )

    parser.add_argument(
        '--save-video',
        action='store_true',
        help='Save output video (video input only)'
    )

    parser.add_argument(
        '--save-frames',
        action='store_true',
        help='Save individual annotated frames (video input only)'
    )

    parser.add_argument(
        '--max-frames',
        type=int,
        default=None,
        help='Maximum number of frames to process (video input only, None = all)'
    )

    # Display arguments
    parser.add_argument(
        '--display',
        action='store_true',
        help='Display video in real-time while processing (video input only)'
    )

    parser.add_argument(
        '--display-scale',
        type=float,
        default=1.0,
        help='Display window scale factor (default: 1.0, e.g. 0.5 for half size)'
    )

    parser.add_argument(
        '--no-original-speed',
        action='store_true',
        help='Process as fast as possible instead of original video speed (when displaying)'
    )

    # Output arguments
    parser.add_argument(
        '--output',
        type=str,
        default='output/',
        help='Output directory (default: output/)'
    )

    parser.add_argument(
        '--save-crop',
        action='store_true',
        help='Save cropped person detections (image input only)'
    )

    parser.add_argument(
        '--crop-dir',
        type=str,
        default='crops/',
        help='Directory for cropped detections (default: crops/)'
    )

    parser.add_argument(
        '--no-show-conf',
        action='store_true',
        help='Hide confidence scores in visualization'
    )

    parser.add_argument(
        '--line-thickness',
        type=int,
        default=2,
        help='Bounding box line thickness (default: 2)'
    )

    args = parser.parse_args()

    # Validate input
    if not Path(args.source).exists():
        logger.error(f"Input file not found: {args.source}")
        return

    # Create output directories
    output_dir = Path(args.output)
    output_dir.mkdir(parents=True, exist_ok=True)

    try:
        # Load model
        model = load_model(args.model, args.device)

        # Check if input is video or image
        if is_video_file(args.source):
            # Video processing
            logger.info("Detected video input")
            stats = process_video(
                model,
                args.source,
                conf_threshold=args.conf,
                imgsz=args.imgsz,
                iou_threshold=args.iou,
                frame_interval=args.frame_interval,
                output_dir=str(output_dir),
                save_video=args.save_video,
                save_frames=args.save_frames,
                max_frames=args.max_frames,
                show_conf=not args.no_show_conf,
                line_thickness=args.line_thickness,
                display=args.display,
                display_scale=args.display_scale,
                original_speed=not args.no_original_speed
            )
        else:
            # Image processing
            logger.info("Detected image input")
            results, image = detect_persons(
                model,
                args.source,
                conf_threshold=args.conf,
                imgsz=args.imgsz,
                iou_threshold=args.iou
            )

            # Generate output filename
            input_name = Path(args.source).stem
            output_path = output_dir / f"{input_name}_detected.jpg"

            # Visualize and save
            annotated_image = visualize_detections(
                image,
                results,
                save_path=str(output_path),
                show_conf=not args.no_show_conf,
                line_thickness=args.line_thickness
            )

            # Save crops if requested
            if args.save_crop:
                save_crops(image, results, args.crop_dir)

            # Print summary
            summary = get_detection_summary(results)
            logger.info("\n" + "="*50)
            logger.info("Detection Summary:")
            logger.info(f"  Total persons detected: {summary['count']}")
            if summary['count'] > 0:
                logger.info(f"  Average confidence: {summary['avg_confidence']:.3f}")
                logger.info(f"  Confidence range: [{min(summary['confidences']):.3f}, {max(summary['confidences']):.3f}]")
            logger.info("="*50)

            logger.info(f"\nResults saved to: {output_path}")

    except Exception as e:
        logger.error(f"Error during detection: {e}")
        raise


if __name__ == '__main__':
    main()
