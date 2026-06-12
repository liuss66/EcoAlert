# Test — 算法测试

轻量级"是否开灯 / 是否有人"检测算法的独立测试环境。

## 算法说明

| 算法 | 方法 | 单帧耗时 | 资源占用 |
|------|------|----------|----------|
| 灯光检测 | ROI 灰度均值 + 磁滞阈值 | < 0.1ms | 极低 |
| 有人检测 | 帧差法 + 降采样（160×120） | < 0.5ms | 极低 |

**设计原则：** 无深度学习模型，纯图像处理，适合边缘设备 / 低配机器部署。

### 灯光检测 (`LightDetector`)

1. 对配置的 `light_rois` 区域计算灰度均值
2. EMA 时间平滑（避免闪烁）
3. 磁滞阈值判定（`on_threshold=140`, `off_threshold=90`）
   - 均值 > 140 → 灯亮
   - 均值 < 90  → 灯灭
   - 中间 → 保持上一状态

### 有人检测 (`MotionDetector`)

1. 输入帧降采样到 160×120（降低 95%+ 计算量）
2. 计算当前帧与上一帧的绝对差
3. 统计差值 > 20 的像素占比
4. 占比 > 3% → 有运动 → 视为有人
5. EMA 时间平滑

### 组合输出 (`SceneAnalyzer`)

```
SceneState {
    person: bool,        # 是否有人
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

### 输出

- 终端实时打印检测状态
- CSV 文件保存到 `Test/output/<video_name>_result.csv`

## 文件结构

```
Test/
├── README.md           # 本文件
├── requirements.txt    # Python 依赖
├── detector.py         # 算法实现（LightDetector, MotionDetector, SceneAnalyzer）
├── run_test.py         # 测试运行器 + 单元测试
└── output/             # CSV 输出目录（自动生成）
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
