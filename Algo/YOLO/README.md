# YOLO WebSocket 检测服务

该服务接收 JPEG 二进制帧，返回 YOLO 人员检测结果，供 EcoAlert 桌面端保持长连接调用。

## 启动

```powershell
pip install -r Algo/YOLO/requirements.txt
python Algo/YOLO/server_ws.py
```

服务默认监听 `0.0.0.0:8090`。模型、设备、阈值及端口在 `config.yaml` 中配置；相对模型路径始终相对于 `Algo/YOLO` 解析，因此可从任意目录启动。

健康检查：

```powershell
Invoke-RestMethod http://localhost:8090/health
```

图片链路测试：

```powershell
python Algo/YOLO/test_server.py --image path/to/test.jpg
```

## WebSocket 协议

- 地址：`ws://host:8090/ws`
- 客户端请求：一条 Binary 消息，内容为完整 JPEG 文件字节
- 服务端成功响应：一条 Text JSON 消息

```json
{
  "count": 1,
  "detections": [
    {
      "class_id": 0,
      "class_name": "person",
      "confidence": 0.91,
      "bbox": [0.5, 0.5, 0.2, 0.6]
    }
  ],
  "process_ms": 32.5
}
```

`bbox` 是归一化的 `[中心 x, 中心 y, 宽, 高]`。单帧处理失败时返回 `{"error":"..."}`，连接保持可用。

## 配置注意事项

- `device: auto` 会优先使用 CUDA，不可用时回退 CPU。
- 同一个模型实例串行执行推理，避免 Ultralytics 模型被多线程并发调用。
- 权重文件位于 `model/`，被 Git 忽略，需要在部署机器单独提供。
