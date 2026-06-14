# EcoAlert MATLAB Algorithm Visualization

本文档介绍如何为EcoAlert的MATLAB检测算法添加可视化功能。

## 可用工具

### 1. MATLAB实时可视化脚本 (`run_video_visualization.m`)

直接在MATLAB环境中运行,提供实时的视频分析可视化界面。

**功能特性:**
- 实时视频帧显示,带ROI区域标注
- 动态图表显示亮度、色度、运动分数
- 灯光状态指示器(ON/OFF)
- 人员检测结果标记
- 实时置信度显示

**使用方法:**
```matlab
% 在MATLAB中运行
cd g:/project/EcoAlert/Algo/Matlab
run_video_visualization
```

**配置选项:**
编辑脚本开头的参数:
```matlab
videoPath = "G:\project\EcoAlert\Video\5·28域控.mp4";  % 视频路径
frameStep = 10;           % 每N帧分析一次
personThreshold = 0.65;   % 人员检测阈值
lightThreshold = 0.70;    % 灯光检测阈值
```

---

### 2. Python交互式Dashboard生成器 (`matlab_visualizer.py`)

通过Python调用MATLAB进行分析,生成交互式HTML仪表盘。

**前置要求:**
- Python 3.7+
- MATLAB已安装并添加到系统PATH
- 无需额外Python包(使用标准库)

**使用方法:**
```bash
cd g:/project/EcoAlert/Algo/Matlab

# 基本用法
python matlab_visualizer.py --video "G:\project\EcoAlert\Video\5·28域控.mp4"

# 自定义参数
python matlab_visualizer.py \
    --video "G:\project\EcoAlert\Video\5·28域控.mp4" \
    --person-threshold 0.65 \
    --light-threshold 0.70 \
    --frame-step 10 \
    --output dashboard.html
```

**命令行参数:**
- `-v, --video`: 视频文件路径(必需)
- `--person-threshold`: 人员检测阈值(默认: 0.65)
- `--light-threshold`: 灯光检测阈值(默认: 0.70)
- `--frame-step`: 帧采样步长(默认: 10)
- `-o, --output`: 输出HTML文件路径(默认: dashboard.html)

**Dashboard功能:**
- 📊 统计卡片:总帧数、检测率、平均值等
- 📈 4个交互式Chart.js图表:
  - 亮度随时间变化
  - 运动分数随时间变化(含人员检测标记)
  - 色度分数随时间变化
  - 检测置信度对比
- 🎬 检测时间轴:可视化展示每帧的人员检测结果
- 🔍 过滤控制:帧采样和人员检测过滤
- 💡 悬停提示:鼠标悬停查看详细数据

---

### 3. 简化调试脚本 (`run_video_debug.m`)

快速输出分析结果到表格或CSV文件。

**使用方法:**
```matlab
% 在MATLAB中运行
run_video_debug

% 导出CSV(取消脚本最后一行的注释)
writetable(resultTable, "ecoalert_matlab_debug.csv");
```

---

## 可视化示例

### 实时可视化界面布局
```
┌─────────────────────────────────┬──────────┐
│     Video Frame with ROI        │  Light   │
│     [检测画面 + ROI框]           │  Status  │
│                                 │  ON/OFF  │
│     Person: YES/NO              │          │
│     Metrics overlay             │ Conf:0.95│
├──────────────┬──────────────────┴──────────┤
│ Brightness   │     Motion Score            │
│ Chart        │     Chart                   │
│ [亮度曲线]   │     [运动曲线+人员标记]      │
├──────────────┴─────────────────────────────┤
│ Color Score  │     Confidence              │
│ Chart        │     Chart                   │
│ [色度曲线]   │     [置信度对比]            │
└──────────────┴─────────────────────────────┘
```

### HTML Dashboard预览
打开生成的`dashboard.html`后可以看到:
- 顶部6个统计卡片
- 过滤控制面板
- 4个可交互图表(支持缩放、悬停查看)
- 底部完整检测时间轴

---

## 性能优化建议

1. **帧采样**:对于长视频,增加`frameStep`值(如20-50)
2. **最大帧数**:设置`maxFramesToAnalyze`限制处理帧数
3. **关闭实时绘图**:在MATLAB中使用`drawnow limitrate`提高性能
4. **批量处理**:使用`run_video_debug.m`导出CSV后用Python分析

---

## 自定义ROI区域

在脚本中配置感兴趣的区域(ROI):

```matlab
roiConfig = EcoAlertDetector.defaultRoiConfig();

% 设置灯光检测区域 (归一化坐标 0-1)
roiConfig.light_rois = EcoAlertDetector.roiRect(0.20, 0.00, 0.60, 0.40, "lamp");

% 调整阈值
roiConfig.light_on_threshold = 0.055;
roiConfig.light_off_threshold = 0.025;
```

---

## 故障排除

### MATLAB未找到
**错误**: `MATLAB not found in PATH`

**解决**:
1. 将MATLAB添加到系统PATH
2. 或使用完整路径调用MATLAB:
   ```bash
   "C:\Program Files\MATLAB\R2023a\bin\matlab.exe" -batch "run('script.m')"
   ```

### 内存不足
**问题**: 处理高分辨率视频时内存溢出

**解决**:
1. 增加帧采样步长(`frameStep`)
2. 降低视频分辨率
3. 限制最大处理帧数

### 图表不更新
**问题**: MATLAB图形界面无响应

**解决**:
1. 使用`drawnow limitrate`而非`drawnow`
2. 减少绘图频率(每N帧更新一次)

---

## 数据导出格式

CSV文件包含以下字段:
- `frame`: 帧编号
- `time_sec`: 时间戳(秒)
- `person`: 人员检测(0/1)
- `light`: 灯光状态(0/1)
- `person_confidence`: 人员置信度(0-1)
- `light_confidence`: 灯光置信度(0-1)
- `light_brightness`: 亮度值(0-255)
- `color_score`: 色度分数(0-0.2)
- `motion_score`: 运动分数(0-1)
- `reason`: 检测原因描述

---

## 扩展开发

### 添加新的可视化指标
1. 在`EcoAlertDetector.m`中添加新的输出字段
2. 在可视化脚本中读取新字段
3. 在HTML Dashboard中添加对应图表

### 集成到Tauri应用
可以参考Python可视化工具的实现,在Rust后端调用MATLAB引擎,在前端React组件中展示结果。

---

## 技术支持

如有问题,请检查:
1. MATLAB版本兼容性(R2020a及以上推荐)
2. 视频文件格式(MP4、AVI等MATLAB支持的格式)
3. 文件路径是否正确(注意转义反斜杠)
