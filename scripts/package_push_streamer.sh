#!/usr/bin/env bash
# 将 Tools/push_streamer 打包到 Release/push_streamer
# 用法：bash scripts/package_push_streamer.sh

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/Tools"
DST="$ROOT/Release/push_streamer"

echo "==> 清理旧目录"
rm -rf "$DST"

echo "==> 创建目录结构"
mkdir -p "$DST/push_streamer"

echo "==> 复制 Python 源码（排除 __pycache__）"
cp "$SRC/push_streamer/__init__.py" "$DST/push_streamer/"
cp "$SRC/push_streamer/__main__.py" "$DST/push_streamer/"
cp "$SRC/push_streamer/cli.py"      "$DST/push_streamer/"
cp "$SRC/push_streamer/push.py"     "$DST/push_streamer/"

echo "==> 复制配置文件和依赖"
cp "$SRC/config.example.yaml" "$DST/"
cp "$SRC/requirements.txt"    "$DST/"

# Release 配置路径需要多退一级：Tools/ → ../Video/，Release/push_streamer/ → ../../Video/
echo "==> 调整 Release 配置中的视频路径（../Video → ../../Video）"
sed -i 's|source: \.\./Video/|source: ../../Video/|g' "$DST/config.example.yaml"

echo "==> 完成"
echo "    输出目录: $DST"
ls -R "$DST"
