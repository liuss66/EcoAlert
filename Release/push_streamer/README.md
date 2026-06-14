# push_streamer · EcoAlert 测试推流器

将本地 MP4 视频文件循环推流为 HLS，供 EcoAlert App 端拉流播放。

## 前置依赖

| 依赖 | 安装方式 |
|------|---------|
| Python 3.10+ | https://www.python.org/downloads/ |
| ffmpeg | `winget install ffmpeg` 或 https://ffmpeg.org/download.html |

## 快速启动

**方式一：双击 `start.bat`**（自动检查依赖并安装）

**方式二：命令行启动**

```bash
cd push_streamer
pip install -r requirements.txt

# 使用配置文件（推荐，8 路固定端点）
python -m push_streamer.cli --config config.example.yaml

# 自动扫描上级目录 Video/ 下所有 .mp4
python -m push_streamer.cli --auto-scan
```

## HLS 端点

启动后访问 `http://127.0.0.1:8080/cam-{N}/index.m3u8`：

| 端点 | 视频文件 | App 源名称 | 分组 |
|------|---------|-----------|------|
| cam-1 | 4·24域控.mp4 | 4·24 域控 | 域控测试视频 |
| cam-5 | 5·27域控.mp4 | 5·27 域控 | 域控测试视频 |
| cam-6 | 5·28域控.mp4 | 5·28 域控 | 域控测试视频 |
| cam-7 | 5·7域控.mp4 | 5·7 域控 | 域控测试视频 |
| cam-2 | 4·24底盘.mp4 | 4·24 底盘 | 底盘测试视频 |
| cam-4 | 5·15底盘.mp4 | 5·15 底盘 | 底盘测试视频 |
| cam-8 | 5·7底盘.mp4 | 5·7 底盘 | 底盘测试视频 |
| cam-3 | 5·14硬件.mp4 | 5·14 硬件 | 硬件测试视频 |

## 与 App 联调

1. 将测试视频 MP4 文件放入仓库根目录 `Video/` 下（与 `Release/` 同级）
2. 运行 `start.bat`
3. 启动 `ecoalert.exe`，登录后进入 **设置 → 调试**
4. 开启 **测试视频源** 开关，8 路 HLS 源自动创建并拉流播放
