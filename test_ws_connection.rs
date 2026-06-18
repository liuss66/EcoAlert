use futures_util::SinkExt;
use std::io::Cursor;
use tokio_tungstenite::connect_async;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageBuffer, Rgb};

#[tokio::main]
async fn main() {
    let url = "ws://localhost:8090/ws";
    println!("[Test] 连接到 {}", url);
    
    match connect_async(url).await {
        Ok((mut ws, _)) => {
            println!("[Test] 连接成功！");
            
            // 创建一个简单的1x1白色图像
            let img = ImageBuffer::<Rgb<u8>, Vec<u8>>::from_raw(
                1, 1, vec![255, 255, 255]
            ).unwrap();
            let dyn_img = DynamicImage::ImageRgb8(img);
            
            let mut buf = Cursor::new(Vec::new());
            let mut encoder = JpegEncoder::new_with_quality(&mut buf, 85);
            
            if let Ok(_) = encoder.encode_image(&dyn_img) {
                let jpeg_bytes = buf.into_inner();
                println!("[Test] 编码完成，发送 {} 字节", jpeg_bytes.len());
                
                match ws.send(tokio_tungstenite::tungstenite::Message::Binary(jpeg_bytes.into())).await {
                    Ok(_) => println!("[Test] 发送成功！"),
                    Err(e) => println!("[Test] 发送失败: {}", e),
                }
                
                // 等待响应
                use futures_util::StreamExt;
                match ws.next().await {
                    Some(Ok(msg)) => println!("[Test] 收到响应: {:?}", msg),
                    Some(Err(e)) => println!("[Test] 接收错误: {}", e),
                    None => println!("[Test] 连接已关闭"),
                }
            }
        }
        Err(e) => println!("[Test] 连接失败: {}", e),
    }
}
