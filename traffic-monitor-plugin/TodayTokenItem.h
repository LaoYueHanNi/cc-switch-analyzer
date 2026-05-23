#pragma once
#include "PluginInterface.h"
#include <string>

/// 显示今日 Token 数的文本项
class CTokenCountItem : public IPluginItem
{
public:
    void SetValue(int64_t tokens, bool connected);
    const wchar_t* GetItemValueText() const override;

private:
    const wchar_t* GetItemName() const override;
    const wchar_t* GetItemId() const override;
    const wchar_t* GetItemLableText() const override;
    const wchar_t* GetItemValueSampleText() const override;

    std::wstring m_value;
};

/// 显示今日 Token 费用的文本项
class CCostItem : public IPluginItem
{
public:
    void SetValue(const std::string& cost, bool connected);
    const wchar_t* GetItemValueText() const override;

private:
    const wchar_t* GetItemName() const override;
    const wchar_t* GetItemId() const override;
    const wchar_t* GetItemLableText() const override;
    const wchar_t* GetItemValueSampleText() const override;

    std::wstring m_value;
};

/// 格式化 Token 数为 K/M/B 格式
std::wstring FormatTokenCount(int64_t tokens);
