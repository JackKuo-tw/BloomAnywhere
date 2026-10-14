#!/bin/bash
DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$DIR" || exit 1

echo "=================================================="
echo " 🌸 正在啟動 BloomAnywhere..."
echo "=================================================="

if [ -f "$DIR/bloomanywhere" ]; then
    exec "$DIR/bloomanywhere"
elif [ -f "$DIR/target/release/bloomanywhere" ]; then
    exec "$DIR/target/release/bloomanywhere"
elif [ -f "$DIR/BloomAnywhere.app/Contents/MacOS/BloomAnywhere" ]; then
    exec "$DIR/BloomAnywhere.app/Contents/MacOS/BloomAnywhere"
else
    echo "正在編譯 Release 版本，請稍候..."
    exec cargo run --release -p bloomanywhere
fi
