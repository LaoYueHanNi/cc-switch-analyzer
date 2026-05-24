#pragma once
#include "PluginInterface.h"
#include "TodayTokenItem.h"
#include <string>

/// CC-Switch Analyzer TrafficMonitor 插件主类（单例）
class CCSwitchAnalyzer : public ITMPlugin
{
private:
    CCSwitchAnalyzer();

public:
    static CCSwitchAnalyzer& Instance();

    // ITMPlugin 接口
    IPluginItem* GetItem(int index) override;
    void DataRequired() override;
    const wchar_t* GetInfo(PluginInfoIndex index) override;
    OptionReturn ShowOptionsDialog(void* hParent) override;
    void OnExtenedInfo(ExtendedInfoIndex index, const wchar_t* data) override;
    void OnInitialize(ITrafficMonitor* pApp) override;

private:
    bool FetchTodayData();
    bool TryConnect(int port);
    bool AutoDiscoverPort();
    void LoadConfig();
    void SaveConfig();

    CTokenCountItem m_token_item;
    CCostItem m_cost_item;
    ITrafficMonitor* m_app{};

    int64_t m_total_tokens{};
    std::string m_total_cost_str{};
    bool m_connected{};
    bool m_discovery_attempted{};

    int m_port{ 19810 };
    std::wstring m_config_dir;

    static CCSwitchAnalyzer m_instance;
};

#ifdef __cplusplus
extern "C" {
#endif
    __declspec(dllexport) ITMPlugin* TMPluginGetInstance();
#ifdef __cplusplus
}
#endif
