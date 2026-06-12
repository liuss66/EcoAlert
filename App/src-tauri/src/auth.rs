// EcoAlert · 鉴权模块
// 使用 SHA-256 + 盐哈希存储密码（生产建议换 argon2，但跨平台无依赖更稳）
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DEFAULT_PASSWORD: &str = "admin123";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub password_hash: String,
    pub salt: String,
    pub updated_at: i64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        let salt = random_salt();
        let password_hash = hash_password(DEFAULT_PASSWORD, &salt);
        Self {
            password_hash,
            salt,
            updated_at: chrono::Utc::now().timestamp(),
        }
    }
}

impl AuthConfig {
    pub fn verify(&self, password: &str) -> bool {
        let h = hash_password(password, &self.salt);
        // 常数时间比较
        constant_time_eq(h.as_bytes(), self.password_hash.as_bytes())
    }

    pub fn change_password(&mut self, new_password: &str) {
        self.salt = random_salt();
        self.password_hash = hash_password(new_password, &self.salt);
        self.updated_at = chrono::Utc::now().timestamp();
    }
}

fn hash_password(password: &str, salt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(b":");
    hasher.update(password.as_bytes());
    let out = hasher.finalize();
    hex_encode(&out)
}

fn random_salt() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut buf);
    hex_encode(&buf)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
