# Tools

把 `Video/` 里的本地视频模拟成"实时"视频源，推流给 App。
让 App 端不需要真实摄像头 / NVR 也能跑完整链路。

## 子目录

- `push_streamer/` — 推流器（Python，依赖 ffmpeg）
  - `cli.py` — 命令行入口
  - `streamer.py` — ffmpeg 进程管理（每个视频源一个 ffmpeg 子进程）
  - `loop_manager.py` — 多路循环调度
  - `overlay.py` — 叠加时间戳 / 通道名 / 模拟码率波动
- `scenario/` — 场景脚本
  - `network_blink.yaml` — 模拟网络抖动 / 短暂掉线
  - `multi_cam.yaml` — 多路并发
- `tests/` — 推流器自测
- `config.example.yaml` — 配置示例

## 快速开始

```bash
cd Tools
pip install -r requirements.txt

# 把一个 30 秒短样本循环推到本地 8080 端口的 HLS
python -m push_streamer.cli \
  --video ../Video/samples/indoor/indoor_720p_30s_meeting_hardware.mp4 \
  --loop \
  --output http://127.0.0.1:8080/cam1/index.m3u8

# 多路
python -m push_streamer.cli --config config.example.yaml
```

App 端只需在「视频源管理」新增一条 `http://127.0.0.1:8080/cam1/index.m3u8` 即可。
