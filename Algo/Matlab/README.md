# EcoAlert MATLAB detector mirror

这里是当前 App 轻量算法的 MATLAB 移植版，方便单步调试阈值、ROI、EMA 和帧差行为。

## 文件

- `EcoAlertDetector.m`: 核心检测器。对应 `App/src-tauri/src/pipeline/detector.rs`。
- `run_video_debug.m`: 视频逐帧/跳帧调试脚本，输出 app 实时卡片同类字段。

## 当前算法边界

- 灯光检测：有 RGB 帧时优先用亮度加权色度分数，默认开灯阈值 `0.055`、关灯阈值 `0.025`。灰度帧才使用亮度阈值兜底。
- 人物检测：目前不是人形模型，而是帧差运动代理。`personThreshold` 会映射为运动面积阈值：`max(clamp(personThreshold, 0.05, 1.0) * 0.03, 0.001)`。
- 状态有记忆：亮度、色度、运动分数都使用 `EMA_ALPHA = 0.3`；灯光开关使用滞回阈值；运动检测依赖上一帧。

## 快速使用

```matlab
detector = EcoAlertDetector(0.65, 0.70);
roiConfig = EcoAlertDetector.defaultRoiConfig();
roiConfig.light_rois = EcoAlertDetector.roiRect(0.20, 0.00, 0.60, 0.40, "lamp");

frame = imread("frame.jpg"); % RGB
result = detector.analyzeScene(frame, roiConfig, "ColorOrder", "rgb");
disp(result.scene);
```

调视频时编辑 `run_video_debug.m` 里的 `videoPath`、`frameStep` 和 ROI，然后运行脚本。

## ROI 格式

ROI 使用和 App 一致的归一化坐标：

```matlab
roiConfig.light_rois = [
    EcoAlertDetector.roiRect(0.25, 0.25, 0.50, 0.50, "lamp")
];
roiConfig.light_on_threshold = 0.055;
roiConfig.light_off_threshold = 0.025;
```

字段也兼容 camelCase：

```matlab
roiConfig.lightRois = EcoAlertDetector.roiRect(0.25, 0.25, 0.50, 0.50);
roiConfig.lightOnThreshold = 0.055;
roiConfig.lightOffThreshold = 0.025;
```

## 与 App 对齐的输出字段

`result.scene` 包含：

- `person`, `light`
- `person_confidence`, `light_confidence`
- `light_brightness`, `color_score`, `motion_score`
- `frame_seq`, `process_ms`, `reason`

`reason` 中的 `light_by_color` 表示使用 RGB 色度判断；`light_by_brightness` 表示使用灰度亮度兜底。
