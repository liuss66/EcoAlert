# Test - App 算法调试环境

`Test/` 现在用于调试 App 主程序中的当前算法链路，旧版独立亮度法 / 光源过滤算法已移除。

## 已迁移的 App 逻辑

| 逻辑 | Test 对应 |
|------|-----------|
| 开灯检测 | RGB 色彩分数优先，灰度亮度兜底 |
| 有人检测 | 帧差运动分数作为当前 App 的临时 person 代理 |
| VLM 补漏 | 可选调用 OpenAI-compatible `/chat/completions`，只在 simple 未检测到人时补 person |
| 报警逻辑 | `!person && light` 进入 suspected，满足 `alarm_hold_sec` 后 active |
| 消除警报 | 按 `recover_policy` 和 `alarm_recover_sec` 恢复为 normal |

## 快速开始

```bash
cd Test
pip install -r requirements.txt
python run_test.py --test
```

## 运行视频

```bash
# 测试 Video/ 下全部视频
python run_test.py

# 测试单个视频
python run_test.py ../Video/5·27域控.mp4

# 每 5 帧采样
python run_test.py --sample 5

# 输出逐帧日志
python run_test.py --verbose

# 生成可视化视频和 CSV
python run_test.py --video-output
```

输出在 `Test/output/`：
- `<video>_result.csv`
- `<video>_analysis.mp4`，需 `--video-output`

## 常用调参

```bash
python run_test.py \
  --person-threshold 0.65 \
  --light-threshold 0.70 \
  --alarm-hold-sec 300 \
  --alarm-recover-sec 60 \
  --recover-policy either
```

ROI 使用归一化坐标：

```bash
python run_test.py --light-roi "0.2,0.0,0.6,0.4" --roi-color-on 0.055 --roi-color-off 0.025
```

## VLM 补漏

```bash
python run_test.py --vlm \
  --vlm-api-base https://your-openai-compatible-host/v1 \
  --vlm-api-key sk-xxx \
  --vlm-model your-vision-model
```

也可用环境变量：

```bash
set ECOALERT_VLM_API_BASE=https://your-openai-compatible-host/v1
set ECOALERT_VLM_API_KEY=sk-xxx
set ECOALERT_VLM_MODEL=your-vision-model
python run_test.py --vlm
```

默认行为与 App 一致：如果 simple 已经判定有人，则跳过 VLM。需要强制每到间隔都跑 VLM 时加：

```bash
python run_test.py --vlm --vlm-no-skip-when-person
```

## 文件

```text
Test/
├── detector.py       # App 算法镜像
├── run_test.py       # 视频/单元测试/可视化运行器
├── requirements.txt
└── output/
```
