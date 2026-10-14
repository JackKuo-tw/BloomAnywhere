#!/bin/bash
DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$DIR" || exit 1

# Automatically clear macOS Gatekeeper quarantine flags
xattr -cr "$DIR" 2>/dev/null

echo "=================================================="
echo " 🌸 正在啟動 BloomAnywhere..."
echo "=================================================="

BIN=""
if [ -f "$DIR/BloomAnywhere.app/Contents/MacOS/BloomAnywhere" ] && [ ! -d "$DIR/crates" ]; then
    BIN="$DIR/BloomAnywhere.app/Contents/MacOS/BloomAnywhere"
elif [ -f "$DIR/target/release/bloomanywhere" ]; then
    BIN="$DIR/target/release/bloomanywhere"
elif [ -f "$DIR/bloomanywhere" ]; then
    BIN="$DIR/bloomanywhere"
fi

if [ -n "$BIN" ]; then
    ARCH=$(uname -m)
    BIN_INFO=$(file "$BIN")
    # If on Apple Silicon but binary is Intel-only and lacks Rosetta
    if [[ "$ARCH" == "arm64" ]] && [[ "$BIN_INFO" != *"arm64"* ]] && [[ "$BIN_INFO" != *"universal"* ]]; then
        if ! /usr/bin/pgrep oahd >/dev/null 2>&1 && ! arch -x86_64 /usr/bin/true 2>/dev/null; then
            echo ""
            echo "❌ 架構不相容提示："
            echo "   您的 Mac 為 Apple Silicon (M1/M2/M3/M4, arm64)，但您下載的是 Intel (x64) 版本，且系統尚未啟用 Rosetta 2 轉譯。"
            echo ""
            echo "👉 請至 GitHub Releases 下載通用版或專用版："
            echo "   1. BloomAnywhere-macos-universal.zip (通用版，所有 Mac 均可直接執行)"
            echo "   2. BloomAnywhere-macos-arm64.zip (Apple Silicon 專用版)"
            echo ""
            echo "   （或於終端機執行 'softwareupdate --install-rosetta' 安裝轉譯支援）"
            echo ""
            read -n 1 -s -r -p "按任意鍵離開..."
            exit 1
        fi
    fi
    exec "$BIN"
else
    echo "正在編譯 Release 版本，請稍候..."
    exec cargo run --release -p bloomanywhere
fi
