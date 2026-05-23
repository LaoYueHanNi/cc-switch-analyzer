#include "CCSwitchAnalyzer.h"
#include <windows.h>
#include <winhttp.h>
#include <cstdio>
#include <cstring>
#include <cstdlib>
#include <string>

#pragma comment(lib, "winhttp.lib")

CCSwitchAnalyzer CCSwitchAnalyzer::m_instance;

CCSwitchAnalyzer::CCSwitchAnalyzer() {}

CCSwitchAnalyzer& CCSwitchAnalyzer::Instance()
{
    return m_instance;
}

IPluginItem* CCSwitchAnalyzer::GetItem(int index)
{
    switch (index) {
    case 0: return &m_token_item;
    case 1: return &m_cost_item;
    default: return nullptr;
    }
}

void CCSwitchAnalyzer::DataRequired()
{
    FetchTodayData();
    m_token_item.SetValue(m_total_tokens, m_connected);
    m_cost_item.SetValue(m_total_cost_str, m_connected);
}

const wchar_t* CCSwitchAnalyzer::GetInfo(PluginInfoIndex index)
{
    switch (index) {
    case TMI_NAME:        return L"CCSwitchAnalyzer";
    case TMI_DESCRIPTION: return L"Today token count and cost from CC-Switch Analyzer";
    case TMI_AUTHOR:      return L"LaoYueHanNi";
    case TMI_COPYRIGHT:   return L"Copyright (C) 2026";
    case TMI_VERSION:     return L"1.0";
    case TMI_URL:         return L"https://github.com/LaoYueHanNi/cc-switch-analyzer";
    default:              return L"";
    }
}

ITMPlugin::OptionReturn CCSwitchAnalyzer::ShowOptionsDialog(void* hParent)
{
    return OR_OPTION_NOT_PROVIDED;
}

void CCSwitchAnalyzer::OnExtenedInfo(ExtendedInfoIndex index, const wchar_t* data)
{
}

void CCSwitchAnalyzer::OnInitialize(ITrafficMonitor* pApp)
{
    m_app = pApp;
}

// ===== WinHTTP 获取今日数据 =====

/// 简单的 JSON 字段提取（从扁平 JSON 中提取数值）
static bool ExtractJsonInt64(const char* json, const char* key, int64_t& out)
{
    char search[64];
    snprintf(search, sizeof(search), "\"%s\":", key);
    const char* pos = strstr(json, search);
    if (!pos) return false;
    pos += strlen(search);
    while (*pos == ' ') pos++;
    out = _strtoi64(pos, nullptr, 10);
    return true;
}

/// 从扁平 JSON 中提取字符串值（去掉引号）
static bool ExtractJsonString(const char* json, const char* key, std::string& out)
{
    char search[64];
    snprintf(search, sizeof(search), "\"%s\":", key);
    const char* pos = strstr(json, search);
    if (!pos) return false;
    pos += strlen(search);
    while (*pos == ' ') pos++;
    if (*pos != '"') return false;
    pos++;
    const char* end = strchr(pos, '"');
    if (!end) return false;
    out.assign(pos, end);
    return true;
}

bool CCSwitchAnalyzer::FetchTodayData()
{
    wchar_t host[] = L"127.0.0.1";
    wchar_t path[64];
    // 动态获取时区偏移（含夏令时）
    TIME_ZONE_INFORMATION tzi;
    DWORD tzResult = GetTimeZoneInformation(&tzi);
    int totalBias = tzi.Bias; // 分钟，UTC+8 时 Bias = -480
    if (tzResult == TIME_ZONE_ID_DAYLIGHT) {
        totalBias += tzi.DaylightBias;
    }
    swprintf_s(path, L"/api/today?tz=%d", -totalBias / 60);

    HINTERNET hSession = WinHttpOpen(L"CCSwitchAnalyzer/1.0",
        WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
        WINHTTP_NO_PROXY_NAME,
        WINHTTP_NO_PROXY_BYPASS, 0);
    if (!hSession) {
        m_connected = false;
        return false;
    }

    DWORD timeout = 2000;
    WinHttpSetOption(hSession, WINHTTP_OPTION_CONNECT_TIMEOUT, &timeout, sizeof(timeout));
    WinHttpSetOption(hSession, WINHTTP_OPTION_RECEIVE_RESPONSE_TIMEOUT, &timeout, sizeof(timeout));

    HINTERNET hConnect = WinHttpConnect(hSession, host, (INTERNET_PORT)m_port, 0);
    if (!hConnect) {
        WinHttpCloseHandle(hSession);
        m_connected = false;
        return false;
    }

    HINTERNET hRequest = WinHttpOpenRequest(hConnect, L"GET", path,
        nullptr, WINHTTP_NO_REFERER,
        WINHTTP_DEFAULT_ACCEPT_TYPES, 0);
    if (!hRequest) {
        WinHttpCloseHandle(hConnect);
        WinHttpCloseHandle(hSession);
        m_connected = false;
        return false;
    }

    BOOL sent = WinHttpSendRequest(hRequest,
        WINHTTP_NO_ADDITIONAL_HEADERS, 0,
        WINHTTP_NO_REQUEST_DATA, 0, 0, 0);
    if (!sent) {
        WinHttpCloseHandle(hRequest);
        WinHttpCloseHandle(hConnect);
        WinHttpCloseHandle(hSession);
        m_connected = false;
        return false;
    }

    BOOL received = WinHttpReceiveResponse(hRequest, nullptr);
    if (!received) {
        WinHttpCloseHandle(hRequest);
        WinHttpCloseHandle(hConnect);
        WinHttpCloseHandle(hSession);
        m_connected = false;
        return false;
    }

    char buffer[1024] = {};
    DWORD bytesRead = 0;
    DWORD totalRead = 0;
    while (WinHttpReadData(hRequest, buffer + totalRead, sizeof(buffer) - totalRead - 1, &bytesRead) && bytesRead > 0) {
        totalRead += bytesRead;
        if (totalRead >= sizeof(buffer) - 1) break;
    }
    buffer[totalRead] = '\0';

    WinHttpCloseHandle(hRequest);
    WinHttpCloseHandle(hConnect);
    WinHttpCloseHandle(hSession);

    if (totalRead == 0) {
        m_connected = false;
        return false;
    }

    if (strstr(buffer, "\"error\"")) {
        m_connected = false;
        return false;
    }

    int64_t totalTokens = 0;
    std::string totalCostStr;
    if (ExtractJsonInt64(buffer, "totalTokens", totalTokens) &&
        ExtractJsonString(buffer, "totalCost", totalCostStr))
    {
        m_total_tokens = totalTokens;
        m_total_cost_str = totalCostStr;
        m_connected = true;
        return true;
    }

    m_connected = false;
    return false;
}

// ===== DLL 导出 =====

ITMPlugin* TMPluginGetInstance()
{
    return &CCSwitchAnalyzer::Instance();
}
