@echo off
setlocal

call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat" x86
if errorlevel 1 exit /b 1

cd /d %~dp0
if not exist build mkdir build

cl /nologo /EHsc /std:c++17 /utf-8 /DUNICODE /D_UNICODE /DNDEBUG /O2 /MT CCSwitchAnalyzer.cpp TodayTokenItem.cpp /Fe:build\CCSwitchAnalyzer.dll /LD /link winhttp.lib /EXPORT:TMPluginGetInstance
if errorlevel 1 exit /b 1

copy /Y build\CCSwitchAnalyzer.dll ..\src-tauri\resources\CCSwitchAnalyzer_x86.dll >nul

endlocal
