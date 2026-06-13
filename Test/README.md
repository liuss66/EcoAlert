# Test — 算法测试

轻量级"是否开灯 / 是否有人"检测算法的独立测试环境。

## 算法说明

| 算法 | 方法 | 单帧耗时 | 资源占用 |
|------|------|----------|----------|
| 灯光检测 | App 使用 RGB 色彩分数；Python 工具保留 ROI 灰度均值 + 磁滞阈值 | < 0.1ms | 极低 |
| 运动检测 | 帧差法 + 降采样（160×120） | < 0.5ms | 极低 |

**设计原则：** 无深度学习模型，纯图像处理，适合边缘设备 / 低配机器部署。

### 灯光检测 (`LightDetector`)

主 App 已切换为优先使用摄像头模式特征：开灯时画面为彩色，关灯后摄像头切红外黑白，Rust 端按 RGB 通道差计算 `color_score`。本 Python 测试工具仍保留早期 ROI 灰度亮度法，用于亮度曲线验证和兜底策略参考。

1. 对配置的 `light_rois` 区域计算灰度均值
2. EMA 时间平滑（避免闪烁）
3. 磁滞阈值判定（`on_threshold=140`, `off_threshold=90`）
   - 均值 > 140 → 灯亮
   - 均值 < 90  → 灯灭
   - 中间 → 保持上一状态

### 运动检测 (`MotionDetector`)

1. 输入帧降采样到 160×120（降低 95%+ 计算量）
2. 计算当前帧与上一帧的绝对差
3. 统计差值 > 20 的像素占比
4. 占比 > 3% → 有运动；仅作为变化信号 / 复核触发参考
5. EMA 时间平滑

注意：主 App 的 Rust 检测链路当前仍会把帧差运动分数作为 `person` 的临时代理输出，目的是让端到端链路和报警状态机可联调；它不是真实人形检测。正式人员状态需要后续接入轻量人形模型或 VLM 复核。Python 测试工具中的 `person` 字段同样不能等同于生产人员检测结论。

### 组合输出 (`SceneAnalyzer`)

```
SceneState {
    person: bool,        # 测试工具中的运动触发标记；主 App 不用帧差直接判定有人
    light: bool,         # 是否开灯
    confidence: float,   # 综合置信度
    light_brightness,    # ROI 平均亮度（0..255）
    motion_score,        # 运动强度（0..1）
    process_ms,          # 单帧耗时
}
```

报警条件：`!person && light` → "无人 + 亮灯"

## 快速开始

### 安装依赖

```bash
cd Test
pip install -r requirements.txt
```

### 运行单元测试（无需视频）

```bash
python run_test.py --test
```

验证算法基本逻辑（合成图像），包含：
- 亮/灭灯场景
- 运动/静止场景
- 磁滞阈值验证
- ROI 区域检测
- 性能基准（720p < 50ms/帧）

### 运行视频测试

```bash
# 测试 Video/ 目录下所有视频
python run_test.py

# 测试单个视频
python run_test.py ../Video/5·27域控.mp4

# 每 5 帧采样（加速 5 倍）
python run_test.py --sample 5

# 详细输出（每帧打印）
python run_test.py --verbose

# 调整阈值（适应不同场景）
python run_test.py --brightness 160 --darkness 100
```

### 生成可视化视频（原视频 + 曲线）

```bash
# 批量处理 Video/ 下所有视频，生成可视化分析视频
python run_test.py --video-output

# 仅处理单个视频
python run_test.py --video-output ../Video/5·27域控.mp4

# 调整曲线面板高度（默认 180px）
python run_test.py --video-output --curve-height 200

# 调整曲线滚动窗口时长（默认 60 秒）
python run_test.py --video-output --window-seconds 120

# 加速生成（每 2 帧采样一次，视频帧率减半）
python run_test.py --video-output --sample 2
```

可视化视频布局：

```
+------------------------------------------+
|        原视频 (1920×1080)                |
+------------------------------------------+
|  灯光亮度  曲线 + 阈值线 + 当前值        |
+------------------------------------------+
|  运动检测  运动分数曲线 + 阈值线         |
+------------------------------------------+
|  报警状态  无人+亮灯时红色高亮           |
+------------------------------------------+
```

性能参考（1920×1080 输入，1920×1620 输出）：
- 渲染耗时：~16ms/帧
- 算法耗时：~1.5ms/帧
- 总耗时：~18ms/帧（> 55fps @ 15fps 源视频，实时以上）

### 输出

- 终端实时打印检测状态
- CSV 文件保存到 `Test/output/<video_name>_result.csv`
- 可视化视频保存到 `Test/output/<video_name>_analysis.mp4`（需 `--video-output`）

## 文件结构

```
Test/
├── README.md           # 本文件
├── requirements.txt    # Python 依赖
├── detector.py         # 算法实现（LightDetector, MotionDetector, SceneAnalyzer）
├── run_test.py         # 测试运行器 + 单元测试 + 可视化视频生成
└── output/             # CSV + 可视化视频 输出目录（自动生成）
```

## 后续集成

算法验证通过后，可移植到 Rust 主应用：

| Python | Rust 对应 |
|--------|-----------|
| `cv2.resize` | `image::imageops::resize` 或手动降采样 |
| `cv2.cvtColor(BGR2GRAY)` | 手动 RGB→Gray 或 `image` crate |
| `np.abs(diff)` | `u8` 迭代差值（已在 `detector.rs` 实现） |
| `region.mean()` | `u64::sum / len`（已在 `detector.rs` 实现） |
| `Rect` ROI | 对应 `store.rs::RoiRect` |
| `SceneState` | 对应 `store.rs::SceneState` |

## 调参建议

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `light_on_threshold` | 140 | 亮度 > 此值 → 灯亮。暗场景调低，亮场景调高 |
| `light_off_threshold` | 90 | 亮度 < 此值 → 灯灭。应与 on 保持 ~50 的间隔 |
| `motion_pixel_thresh` | 20 | 单像素差值阈值。光照变化大时调高 |
| `motion_area_thresh` | 0.03 | 运动面积比阈值。小物体运动多时调低 |
| `smooth_alpha` | 0.3 | EMA 平滑系数。越小越平滑，响应越慢 |
