//! 渠道 Token 管理
//!
//! 各平台（飞书 / 企业微信 / QQ）的 access_token 获取与缓存。
//! Token 存在 NotificationTarget 的 access_token / token_expires_at 字段中。

use crate::store::NotificationTarget;
use reqwest::Client;
use serde::Deserialize;
use std::sync::OnceLock;

static AUTH_CLIENT: OnceLock<Client> = OnceLock::new();

fn auth_client() -> &'static Client {
    AUTH_CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("auth http client")
    })
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    app_access_token: Option<String>,
    tenant_access_token: Option<String>,
    expires_in: Option<i64>,
    expire: Option<i64>,
    // 错误字段（各平台不同）
    code: Option<i64>,
    errcode: Option<i64>,
    msg: Option<String>,
    errmsg: Option<String>,
}

/// 确保 target 有可用的 access_token，过期则自动刷新
pub async fn ensure_access_token(target: &mut NotificationTarget) -> Result<String, String> {
    if !target.token_needs_refresh() {
        return Ok(target.access_token.clone());
    }
    let (token, expires_in) = match target.channel_type.as_str() {
        "feishu" => fetch_feishu_token(&target.app_id, &target.app_secret).await?,
        "wechat_work" => fetch_wechat_work_token(&target.app_id, &target.app_secret).await?,
        "qqbot" => fetch_qq_token(&target.app_id, &target.app_secret).await?,
        other => return Err(format!("不支持的渠道类型: {other}")),
    };
    target.access_token = token.clone();
    target.token_expires_at = chrono::Utc::now().timestamp() + expires_in;
    Ok(token)
}

/// 飞书：获取 app_access_token
/// POST https://open.feishu.cn/open-apis/auth/v3/app_access_token/internal
async fn fetch_feishu_token(app_id: &str, app_secret: &str) -> Result<(String, i64), String> {
    let resp = auth_client()
        .post("https://open.feishu.cn/open-apis/auth/v3/app_access_token/internal")
        .json(&serde_json::json!({
            "app_id": app_id,
            "app_secret": app_secret,
        }))
        .send()
        .await
        .map_err(|e| format!("飞书 token 请求失败: {e}"))?;
    let body: TokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("飞书 token 解析失败: {e}"))?;
    let code = body.code.unwrap_or(0);
    if code != 0 {
        return Err(format!(
            "飞书 token 错误: {} (code={})",
            body.msg.as_deref().unwrap_or("unknown"),
            code
        ));
    }
    let token = body
        .app_access_token
        .or(body.tenant_access_token)
        .or(body.access_token)
        .ok_or("飞书返回中无 access_token")?;
    let expires = body.expire.or(body.expires_in).unwrap_or(7200);
    Ok((token, expires))
}

/// 企业微信：获取 access_token
/// GET https://qyapi.weixin.qq.com/cgi-bin/gettoken?corpid=xx&corpsecret=xx
async fn fetch_wechat_work_token(corp_id: &str, secret: &str) -> Result<(String, i64), String> {
    let url = format!(
        "https://qyapi.weixin.qq.com/cgi-bin/gettoken?corpid={}&corpsecret={}",
        urlencoded(corp_id),
        urlencoded(secret),
    );
    let resp = auth_client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("企微 token 请求失败: {e}"))?;
    let body: TokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("企微 token 解析失败: {e}"))?;
    let code = body.errcode.or(body.code).unwrap_or(0);
    if code != 0 {
        return Err(format!(
            "企微 token 错误: {} (code={})",
            body.errmsg
                .as_deref()
                .or(body.msg.as_deref())
                .unwrap_or("unknown"),
            code
        ));
    }
    let token = body.access_token.ok_or("企微返回中无 access_token")?;
    let expires = body.expires_in.unwrap_or(7200);
    Ok((token, expires))
}

/// QQ Bot：获取 access_token
/// POST https://bots.qq.com/app/getAppAccessToken
async fn fetch_qq_token(app_id: &str, client_secret: &str) -> Result<(String, i64), String> {
    let resp = auth_client()
        .post("https://bots.qq.com/app/getAppAccessToken")
        .json(&serde_json::json!({
            "appId": app_id,
            "clientSecret": client_secret,
        }))
        .send()
        .await
        .map_err(|e| format!("QQ token 请求失败: {e}"))?;
    let body: TokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("QQ token 解析失败: {e}"))?;
    let token = body.access_token.ok_or("QQ 返回中无 access_token")?;
    let expires = body.expires_in.unwrap_or(7200);
    Ok((token, expires))
}

/// 验证凭证是否有效（获取一次 token 试一下）
pub async fn verify_credentials(
    channel_type: &str,
    app_id: &str,
    app_secret: &str,
) -> Result<(), String> {
    match channel_type {
        "feishu" => {
            let _ = fetch_feishu_token(app_id, app_secret).await?;
        }
        "wechat_work" => {
            let _ = fetch_wechat_work_token(app_id, app_secret).await?;
        }
        "qqbot" => {
            let _ = fetch_qq_token(app_id, app_secret).await?;
        }
        other => return Err(format!("不支持的渠道: {other}")),
    }
    Ok(())
}

/// 简易 URL encode
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
