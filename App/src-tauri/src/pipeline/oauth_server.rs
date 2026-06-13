//! 飞书 OAuth 本地回调 Server
//!
//! 流程：
//! 1. 启动本地 HTTP Server（127.0.0.1:随机端口）
//! 2. 生成 OAuth URL（redirect_uri = http://127.0.0.1:PORT/callback）
//! 3. 前端生成二维码显示
//! 4. 用户扫码 → 飞书回调到本地 → 拿到 auth_code
//! 5. 用 auth_code 换 user_access_token

use parking_lot::Mutex;
use std::sync::Arc;
use tiny_http::{Header, Response, Server, StatusCode};

/// OAuth 绑定会话
pub struct OAuthSession {
    pub port: u16,
    pub state: String,
    pub auth_code: Arc<Mutex<Option<String>>>,
    pub done: Arc<Mutex<bool>>,
    server_handle: Option<std::thread::JoinHandle<()>>,
}

impl OAuthSession {
    /// 启动本地 HTTP Server，返回 session
    pub fn start() -> Result<Self, String> {
        // 先用 TcpListener 拿一个空闲端口
        let probe =
            std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| format!("分配端口失败: {e}"))?;
        let port = probe
            .local_addr()
            .map_err(|e| format!("获取端口失败: {e}"))?
            .port();
        drop(probe); // 释放端口让 tiny_http 用

        let bind = format!("127.0.0.1:{port}");
        let server = Server::http(&bind).map_err(|e| format!("启动本地 HTTP Server 失败: {e}"))?;

        let state = uuid::Uuid::new_v4().simple().to_string();
        let auth_code = Arc::new(Mutex::new(None));
        let done = Arc::new(Mutex::new(false));

        let auth_code_clone = auth_code.clone();
        let done_clone = done.clone();

        let server_handle = std::thread::spawn(move || {
            for request in server.incoming_requests() {
                let url = request.url().to_string();

                if url.starts_with("/callback") {
                    // 解析 query 参数
                    let query = url.split('?').nth(1).unwrap_or("");
                    let params: std::collections::HashMap<&str, &str> = query
                        .split('&')
                        .filter_map(|pair| {
                            let mut parts = pair.splitn(2, '=');
                            Some((parts.next()?, parts.next().unwrap_or("")))
                        })
                        .collect();

                    if let Some(code) = params.get("code") {
                        *auth_code_clone.lock() = Some(code.to_string());
                        *done_clone.lock() = true;

                        // 返回成功页面
                        let html = SUCCESS_HTML;
                        let header =
                            Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap();
                        let _ = request.respond(
                            Response::from_string(html)
                                .with_header(header)
                                .with_status_code(StatusCode(200)),
                        );
                    } else {
                        let _ = request.respond(
                            Response::from_string("Missing code parameter")
                                .with_status_code(StatusCode(400)),
                        );
                    }
                } else if url.starts_with("/status") {
                    let is_done = *done_clone.lock();
                    let body = if is_done {
                        r#"{"status":"success"}"#
                    } else {
                        r#"{"status":"pending"}"#
                    };
                    let header = Header::from_bytes("Content-Type", "application/json").unwrap();
                    let _ = request.respond(
                        Response::from_string(body)
                            .with_header(header)
                            .with_status_code(StatusCode(200)),
                    );
                } else {
                    let _ = request.respond(
                        Response::from_string("EcoAlert OAuth Callback Server")
                            .with_status_code(StatusCode(200)),
                    );
                }
            }
        });

        Ok(Self {
            port,
            state,
            auth_code,
            done,
            server_handle: Some(server_handle),
        })
    }

    /// 生成飞书 OAuth 授权 URL
    pub fn feishu_auth_url(&self, app_id: &str) -> String {
        let redirect_uri = format!("http://127.0.0.1:{}/callback", self.port);
        let redirect_uri_encoded = urlencoded(&redirect_uri);
        format!(
            "https://accounts.feishu.cn/open-apis/authen/v1/authorize?\
             client_id={}&redirect_uri={}&state={}&scope=im:message+im:chat+im:chat:read&response_type=code",
            app_id, redirect_uri_encoded, self.state
        )
    }

    /// 获取授权码（阻塞等待，超时秒数）
    pub fn wait_for_code(&self, timeout_secs: u32) -> Result<String, String> {
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs as u64);
        loop {
            if let Some(code) = self.auth_code.lock().clone() {
                return Ok(code);
            }
            if std::time::Instant::now() > deadline {
                return Err("OAuth 授权超时，请重试".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    /// 检查是否已完成授权
    pub fn is_done(&self) -> bool {
        *self.done.lock()
    }

    /// 停止 server
    pub fn stop(mut self) {
        // Drop the server to stop accepting new connections
        self.server_handle.take();
    }
}

/// 用 auth_code 换 user_access_token
pub async fn exchange_feishu_token(
    app_id: &str,
    app_secret: &str,
    auth_code: &str,
) -> Result<(String, i64), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let resp = client
        .post("https://open.feishu.cn/open-apis/authen/v2/oauth/token")
        .json(&serde_json::json!({
            "grant_type": "authorization_code",
            "client_id": app_id,
            "client_secret": app_secret,
            "code": auth_code,
        }))
        .send()
        .await
        .map_err(|e| format!("飞书 token 交换失败: {e}"))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("飞书 token 响应解析失败: {e}"))?;

    let code = body["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        return Err(format!(
            "飞书 token 错误: {} (code={})",
            body["message"].as_str().unwrap_or("unknown"),
            code
        ));
    }

    let token = body["access_token"]
        .as_str()
        .ok_or("飞书返回中无 access_token")?
        .to_string();
    let expires = body["expires_in"].as_i64().unwrap_or(7200);
    Ok((token, expires))
}

/// 获取飞书机器人所在的群列表（需要 user_access_token）
pub async fn list_feishu_chats(access_token: &str) -> Result<Vec<FeishuChat>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let resp = client
        .get("https://open.feishu.cn/open-apis/im/v1/chats?page_size=50")
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
        .await
        .map_err(|e| format!("飞书群列表请求失败: {e}"))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("飞书群列表解析失败: {e}"))?;

    let code = body["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        return Err(format!(
            "飞书群列表错误: {} (code={})",
            body["msg"].as_str().unwrap_or("unknown"),
            code
        ));
    }

    let items = body["data"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let chats = items
        .iter()
        .filter_map(|item| {
            Some(FeishuChat {
                chat_id: item["chat_id"].as_str()?.to_string(),
                name: item["name"].as_str().unwrap_or("(未命名)").to_string(),
            })
        })
        .collect();

    Ok(chats)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FeishuChat {
    pub chat_id: String,
    pub name: String,
}

fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

const SUCCESS_HTML: &str = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>授权成功</title>
<style>
body { font-family: -apple-system, sans-serif; display: flex; align-items: center; justify-content: center; height: 100vh; background: #f0fdf4; }
.card { text-align: center; padding: 40px; background: white; border-radius: 16px; box-shadow: 0 4px 24px rgba(0,0,0,0.1); }
.icon { font-size: 48px; margin-bottom: 16px; }
h2 { color: #166534; margin-bottom: 8px; }
p { color: #6b7280; }
</style></head><body>
<div class="card">
  <div class="icon">✅</div>
  <h2>授权成功</h2>
  <p>已绑定到 EcoAlert，请返回应用继续操作</p>
</div></body></html>"#;
