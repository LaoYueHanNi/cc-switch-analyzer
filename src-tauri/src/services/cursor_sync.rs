use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::utils;

const CURSOR_HTTP_TIMEOUT: Duration = Duration::from_secs(8);
/// 自动同步缓存有效期：24 小时内不重复请求 Cursor API（手动同步不受限）
pub const CURSOR_AUTO_SYNC_FRESHNESS: Duration = Duration::from_secs(24 * 60 * 60);

const USAGE_CSV_ENDPOINT: &str =
    "https://cursor.com/api/dashboard/export-usage-events-csv?strategy=tokens";
const USAGE_SUMMARY_ENDPOINT: &str = "https://cursor.com/api/usage-summary";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorCredentials {
    #[serde(rename = "sessionToken")]
    pub session_token: String,
    #[serde(rename = "userId", skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncCursorResult {
    pub synced: bool,
    pub rows: usize,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateSessionResult {
    pub valid: bool,
    pub membership_type: Option<String>,
    pub error: Option<String>,
}

fn build_http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(CURSOR_HTTP_TIMEOUT))
        .max_redirects(5)
        .build()
        .into()
}

fn extract_user_id_from_session_token(token: &str) -> Option<String> {
    let token = token.trim();
    let user_id = if token.contains("%3A%3A") {
        token.split("%3A%3A").next()?
    } else if token.contains("::") {
        token.split("::").next()?
    } else {
        return None;
    };
    let user_id = user_id.trim();
    if user_id.is_empty() {
        None
    } else {
        Some(user_id.to_string())
    }
}

fn atomic_write_file(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
    }
    let temp_path = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&temp_path)
            .map_err(|e| format!("创建临时文件失败: {}", e))?;
        file.write_all(contents.as_bytes())
            .map_err(|e| format!("写入临时文件失败: {}", e))?;
    }
    if let Err(err) = fs::rename(&temp_path, path) {
        if path.exists() {
            fs::copy(&temp_path, path).map_err(|copy_err| {
                let _ = fs::remove_file(&temp_path);
                format!("持久化文件失败: rename={}, copy={}", err, copy_err)
            })?;
            let _ = fs::remove_file(&temp_path);
        } else {
            let _ = fs::remove_file(&temp_path);
            return Err(format!("持久化文件失败: {}", err));
        }
    }
    Ok(())
}

pub fn load_credentials() -> Option<CursorCredentials> {
    let path = utils::get_cursor_credentials_path().ok()?;
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn save_credentials(session_token: &str) -> Result<CursorCredentials, String> {
    let token = session_token.trim();
    if token.is_empty() {
        return Err("Session Token 不能为空".to_string());
    }
    let creds = CursorCredentials {
        session_token: token.to_string(),
        user_id: extract_user_id_from_session_token(token),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let path = utils::get_cursor_credentials_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {}", e))?;
    }
    let json = serde_json::to_string_pretty(&creds)
        .map_err(|e| format!("序列化凭证失败: {}", e))?;
    atomic_write_file(&path, &json)?;
    Ok(creds)
}

pub fn clear_credentials() -> Result<(), String> {
    let path = utils::get_cursor_credentials_path()?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("删除凭证失败: {}", e))?;
    }
    Ok(())
}

pub fn is_logged_in() -> bool {
    load_credentials().is_some()
}

pub fn validate_cursor_session(session_token: &str) -> ValidateSessionResult {
    let agent = build_http_agent();

    let response = match agent
        .get(USAGE_SUMMARY_ENDPOINT)
        .header("Cookie", &format!("WorkosCursorSessionToken={}", session_token.trim()))
        .header("Referer", "https://www.cursor.com/settings")
        .header("Accept", "*/*")
        .call()
    {
        Ok(resp) => resp,
        Err(ureq::Error::StatusCode(code)) => {
            if code == 401 || code == 403 {
                return ValidateSessionResult {
                    valid: false,
                    membership_type: None,
                    error: Some("Session Token 已过期或无效".to_string()),
                };
            }
            return ValidateSessionResult {
                valid: false,
                membership_type: None,
                error: Some(format!("Cursor API 返回状态 {}", code)),
            };
        }
        Err(e) => {
            return ValidateSessionResult {
                valid: false,
                membership_type: None,
                error: Some(format!("请求失败: {}", e)),
            };
        }
    };

    let body = match response.into_body().read_to_string() {
        Ok(body) => body,
        Err(e) => {
            return ValidateSessionResult {
                valid: false,
                membership_type: None,
                error: Some(format!("读取响应失败: {}", e)),
            };
        }
    };

    let data: serde_json::Value = match serde_json::from_str(&body) {
        Ok(data) => data,
        Err(e) => {
            return ValidateSessionResult {
                valid: false,
                membership_type: None,
                error: Some(format!("解析响应失败: {}", e)),
            };
        }
    };

    let has_billing_start = data
        .get("billingCycleStart")
        .and_then(|v| v.as_str())
        .is_some();
    let has_billing_end = data
        .get("billingCycleEnd")
        .and_then(|v| v.as_str())
        .is_some();

    if has_billing_start && has_billing_end {
        ValidateSessionResult {
            valid: true,
            membership_type: data
                .get("membershipType")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            error: None,
        }
    } else {
        ValidateSessionResult {
            valid: false,
            membership_type: None,
            error: Some("Cursor API 响应格式无效".to_string()),
        }
    }
}

pub fn fetch_cursor_usage_csv(session_token: &str) -> Result<String, String> {
    let agent = build_http_agent();
    let response = agent
        .get(USAGE_CSV_ENDPOINT)
        .header("Cookie", &format!("WorkosCursorSessionToken={}", session_token.trim()))
        .header("Referer", "https://www.cursor.com/settings")
        .header("Accept", "*/*")
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(code) if code == 401 || code == 403 => {
                "Cursor Session Token 已过期，请重新登录".to_string()
            }
            other => format!("Cursor API 请求失败: {}", other),
        })?;

    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("读取 Cursor CSV 响应失败: {}", e))?;

    if !body.starts_with("Date,") {
        return Err("Cursor API 返回非 CSV 格式".to_string());
    }
    Ok(body)
}

pub fn count_cursor_csv_rows(csv_text: &str) -> usize {
    csv_text.lines().skip(1).filter(|l| !l.trim().is_empty()).count()
}

pub fn cache_is_fresh() -> bool {
    let Ok(path) = utils::get_cursor_usage_csv_path() else {
        return false;
    };
    let Ok(meta) = fs::metadata(&path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    modified
        .elapsed()
        .map(|elapsed| elapsed < CURSOR_AUTO_SYNC_FRESHNESS)
        .unwrap_or(false)
}

pub fn cache_last_modified() -> Option<SystemTime> {
    let path = utils::get_cursor_usage_csv_path().ok()?;
    fs::metadata(&path).ok()?.modified().ok()
}

pub fn sync_cursor_cache() -> SyncCursorResult {
    let creds = match load_credentials() {
        Some(c) => c,
        None => {
            return SyncCursorResult {
                synced: false,
                rows: 0,
                error: Some("未登录 Cursor".to_string()),
            };
        }
    };

    match fetch_cursor_usage_csv(&creds.session_token) {
        Ok(csv_text) => {
            let cache_dir = match utils::get_cursor_cache_dir() {
                Ok(dir) => dir,
                Err(e) => {
                    return SyncCursorResult {
                        synced: false,
                        rows: 0,
                        error: Some(e),
                    };
                }
            };
            if let Err(e) = fs::create_dir_all(&cache_dir) {
                return SyncCursorResult {
                    synced: false,
                    rows: 0,
                    error: Some(format!("创建缓存目录失败: {}", e)),
                };
            }
            let file_path = cache_dir.join("usage.csv");
            let row_count = count_cursor_csv_rows(&csv_text);
            if let Err(e) = atomic_write_file(&file_path, &csv_text) {
                return SyncCursorResult {
                    synced: false,
                    rows: 0,
                    error: Some(e),
                };
            }
            SyncCursorResult {
                synced: true,
                rows: row_count,
                error: None,
            }
        }
        Err(e) => SyncCursorResult {
            synced: false,
            rows: 0,
            error: Some(e),
        },
    }
}

/// 缓存过期时自动同步，返回是否执行了同步
pub fn maybe_auto_sync() -> Result<bool, String> {
    if !is_logged_in() {
        return Ok(false);
    }
    if cache_is_fresh() {
        return Ok(false);
    }
    let result = sync_cursor_cache();
    if result.synced {
        Ok(true)
    } else if let Some(err) = result.error {
        log::warn!("[CURSOR] 自动同步失败: {}", err);
        Ok(false)
    } else {
        Ok(false)
    }
}
