@echo off
chcp 65001 >nul
cd /d "%~dp0"
title BloomAnywhere (Bloom Mode)

echo ==================================================
echo  🌸 正在啟動 BloomAnywhere...
echo ==================================================

if exist "BloomAnywhere.exe" (
    BloomAnywhere.exe
) else if exist "target\release\bloomanywhere.exe" (
    target\release\bloomanywhere.exe
) else if exist "bloomanywhere.exe" (
    bloomanywhere.exe
) else (
    echo 首次啟動，正在編譯 Release 版本，請稍候...
    cargo run --release -p bloomanywhere
)
pause
