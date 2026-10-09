@echo off
setlocal
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat" x64
if errorlevel 1 exit /b 1
cd /d %~dp0
if not exist build mkdir build
cl /nologo /EHsc /std:c++17 /utf-8 /DUNICODE /D_UNICODE /O2 /MT tests\reconnect.cpp CCSwitchAnalyzer.cpp TodayTokenItem.cpp /Fe:build\reconnect-test.exe /link winhttp.lib ws2_32.lib
if errorlevel 1 exit /b 1
build\reconnect-test.exe
exit /b %errorlevel%
