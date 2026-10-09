// Native integration regression: real WinHTTP failures and repeated port discovery.
#include <winsock2.h>
#include <windows.h>
#include <cstdint>
#include <string>
#include <thread>
#include <fstream>
#include <iostream>
#include "../CCSwitchAnalyzer.h"

class FixtureServer {
public:
    SOCKET listener{ INVALID_SOCKET };
    int port{};
    std::thread worker;

    bool reserve(int p) {
        listener = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
        BOOL exclusive = TRUE;
        setsockopt(listener, SOL_SOCKET, SO_EXCLUSIVEADDRUSE, reinterpret_cast<const char*>(&exclusive), sizeof(exclusive));
        sockaddr_in addr{};
        addr.sin_family = AF_INET;
        addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        addr.sin_port = htons(static_cast<u_short>(p));
        if (bind(listener, reinterpret_cast<sockaddr*>(&addr), sizeof(addr)) == SOCKET_ERROR) {
            closesocket(listener); listener = INVALID_SOCKET; return false;
        }
        port = p;
        return true;
    }

    void start(int tokens) {
        listen(listener, 8);
        const SOCKET server = listener;
        worker = std::thread([server, tokens] {
            for (;;) {
                SOCKET client = accept(server, nullptr, nullptr);
                if (client == INVALID_SOCKET) break;
                char buffer[1024]{};
                DWORD timeout = 1000;
                setsockopt(client, SOL_SOCKET, SO_RCVTIMEO, reinterpret_cast<const char*>(&timeout), sizeof(timeout));
                const int count = recv(client, buffer, sizeof(buffer) - 1, 0);
                if (count > 0) {
                    const bool ping = std::string(buffer).find("/api/ping") != std::string::npos;
                    const std::string body = ping ? "{\"status\":\"ok\"}" :
                        "{\"totalTokens\":" + std::to_string(tokens) + ",\"totalCost\":\"1.00\",\"stale\":false}";
                    const std::string reply = "HTTP/1.1 200 OK\r\nContent-Length: " + std::to_string(body.size()) +
                        "\r\nConnection: close\r\n\r\n" + body;
                    send(client, reply.data(), static_cast<int>(reply.size()), 0);
                }
                closesocket(client);
            }
        });
    }

    void stop() {
        if (listener != INVALID_SOCKET) { closesocket(listener); listener = INVALID_SOCKET; }
        if (worker.joinable()) worker.join();
    }
    ~FixtureServer() { stop(); }
};

static bool wait_for(CCSwitchAnalyzer& plugin, const wchar_t* expected) {
    const ULONGLONG deadline = GetTickCount64() + 15000;
    do {
        plugin.DataRequired();
        if (std::wstring(plugin.GetItem(0)->GetItemValueText()) == expected) return true;
        Sleep(300);
    } while (GetTickCount64() < deadline);
    return false;
}

int main() {
    WSADATA data{};
    if (WSAStartup(MAKEWORD(2, 2), &data) != 0) return 1;
    FixtureServer first, second, closed;
    for (int port = 19810; port <= 19820; ++port) {
        if (closed.listener == INVALID_SOCKET) { closed.reserve(port); continue; }
        if (first.listener == INVALID_SOCKET) { first.reserve(port); continue; }
        if (second.reserve(port)) break;
    }
    if (second.listener == INVALID_SOCKET) {
        std::cerr << "Three free fixture ports required in 19810-19820\n"; return 1;
    }
    wchar_t temp[MAX_PATH]{};
    GetTempPathW(MAX_PATH, temp);
    std::wstring config = std::wstring(temp) + L"ccsa-plugin-regression-" + std::to_wstring(GetCurrentProcessId()) + L"\\";
    CreateDirectoryW(config.c_str(), nullptr);
    std::ofstream(config + L"port.ini") << "port=" << closed.port << '\n';
    // A bound socket without listen() reliably produces SendRequest failure even
    // though WinHttpConnect returns a handle, the original regression trigger.
    auto& plugin = CCSwitchAnalyzer::Instance();
    plugin.OnExtenedInfo(ITMPlugin::EI_CONFIG_DIR, config.c_str());
    plugin.OnInitialize(nullptr);
    first.start(123);
    if (!wait_for(plugin, L"123")) { std::cerr << "First discovery failed\n"; return 1; }
    first.stop();
    second.start(456);
    if (!wait_for(plugin, L"456")) { std::cerr << "Second outage recovery failed\n"; return 1; }
    second.stop();
    closed.stop();
    DeleteFileW((config + L"port.ini").c_str());
    RemoveDirectoryW(config.c_str());
    WSACleanup();
    std::cout << "WinHTTP closed-port discovery and second outage recovery passed\n";
}
