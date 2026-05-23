cmake_minimum_required(VERSION 3.15)
project(CCSwitchAnalyzer LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# 使用静态 MFC
set(CMAKE_MFC_FLAG 2)

add_library(CCSwitchAnalyzer SHARED
    CCSwitchAnalyzer.cpp
    CCSwitchAnalyzer.h
    TodayTokenItem.cpp
    TodayTokenItem.h
    resource.h
)

# 链接 WinHTTP
target_link_libraries(CCSwitchAnalyzer PRIVATE winhttp)

# 定义 UNICODE
target_compile_definitions(CCSwitchAnalyzer PRIVATE UNICODE _UNICODE)

# DLL 输出设置
set_target_properties(CCSwitchAnalyzer PROPERTIES
    OUTPUT_NAME "CCSwitchAnalyzer"
    SUFFIX ".dll"
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
)
