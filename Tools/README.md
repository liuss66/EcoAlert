# Tools

开发辅助工具目录。当前只保留推流器占位入口和示例配置，真实 ffmpeg 推流逻辑还没实现。

## 子目录

- `push_streamer/cli.py` — 推流器命令行占位入口
- `config.example.yaml` — 配置示例
- `scenario/` — 后续推流器实现后的场景配置

## 快速开始

```bash
cd Tools
pip install -r requirements.txt

# 当前只会打印参数；ffmpeg 推流实现待补
python -m push_streamer.cli \
  --video ../Video/sample_01_meeting.mp4 \
  --loop \
  --output http://127.0.0.1:8080/cam1/index.m3u8

# 多路配置示例
python -m push_streamer.cli --config config.example.yaml
```

推流器补完后，App 端可在「视频管理」新增 `http://127.0.0.1:8080/cam1/index.m3u8`。
