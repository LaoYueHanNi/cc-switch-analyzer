use std::time::Duration;

use crate::models::CloudPricingData;

const TIMEOUT_SECS: u64 = 5;

/// 从云端拉取定价 JSON 并解析
pub fn fetch_cloud_pricing(url: &str) -> Result<CloudPricingData, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(TIMEOUT_SECS)))
        .max_redirects(5)
        .build()
        .into();

    let mut response = agent
        .get(url)
        .call()
        .map_err(|e| format!("云端定价请求失败: {}", e))?;

    let body: String = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("读取云端定价响应失败: {}", e))?;

    parse_cloud_pricing(&body)
}

/// 解析云端定价 JSON
pub fn parse_cloud_pricing(json_str: &str) -> Result<CloudPricingData, String> {
    serde_json::from_str(json_str).map_err(|e| format!("解析云端定价 JSON 失败: {}", e))
}
