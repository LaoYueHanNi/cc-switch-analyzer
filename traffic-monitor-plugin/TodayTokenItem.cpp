#include "TodayTokenItem.h"
#include <windows.h>
#include <cstdio>
#include <cwchar>

// ===== Token 数格式化: <1K 原数字, <1M K, <1G M, >=1G B =====

std::wstring FormatTokenCount(int64_t tokens)
{
    if (tokens < 0) return L"--";

    wchar_t buf[32];
    if (tokens < 1000LL) {
        swprintf_s(buf, L"%lld", tokens);
    } else if (tokens < 1000000LL) {
        double k = tokens / 1000.0;
        if (k < 10.0)
            swprintf_s(buf, L"%.1fK", k);
        else
            swprintf_s(buf, L"%.0fK", k);
    } else if (tokens < 1000000000LL) {
        double m = tokens / 1000000.0;
        if (m < 10.0)
            swprintf_s(buf, L"%.2fM", m);
        else if (m < 100.0)
            swprintf_s(buf, L"%.1fM", m);
        else
            swprintf_s(buf, L"%.0fM", m);
    } else {
        double b = tokens / 1000000000.0;
        if (b < 10.0)
            swprintf_s(buf, L"%.2fB", b);
        else
            swprintf_s(buf, L"%.1fB", b);
    }
    return buf;
}

// ===== CTokenCountItem =====

void CTokenCountItem::SetValue(int64_t tokens, bool connected)
{
    m_value = connected ? FormatTokenCount(tokens) : L"--";
}

const wchar_t* CTokenCountItem::GetItemName() const
{
    return L"Today Tokens";
}

const wchar_t* CTokenCountItem::GetItemId() const
{
    return L"ccsw_tk_01";
}

const wchar_t* CTokenCountItem::GetItemLableText() const
{
    return L"Tokens";
}

const wchar_t* CTokenCountItem::GetItemValueText() const
{
    return m_value.c_str();
}

const wchar_t* CTokenCountItem::GetItemValueSampleText() const
{
    return L"999.9M";
}

// ===== CCostItem =====

void CCostItem::SetValue(const std::string& cost, bool connected)
{
    if (!connected || cost.empty()) { m_value = L"--"; return; }
    int len = MultiByteToWideChar(CP_UTF8, 0, cost.c_str(), -1, nullptr, 0);
    if (len <= 0) { m_value = L"--"; return; }
    m_value.resize(len - 1);
    MultiByteToWideChar(CP_UTF8, 0, cost.c_str(), -1, &m_value[0], len);
}

const wchar_t* CCostItem::GetItemName() const
{
    return L"Today Cost";
}

const wchar_t* CCostItem::GetItemId() const
{
    return L"ccsw_cst_01";
}

const wchar_t* CCostItem::GetItemLableText() const
{
    return L"Cost";
}

const wchar_t* CCostItem::GetItemValueText() const
{
    return m_value.c_str();
}

const wchar_t* CCostItem::GetItemValueSampleText() const
{
    return L"172.57¥";
}
