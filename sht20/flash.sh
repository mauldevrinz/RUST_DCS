#!/bin/bash

# ESP32 Flash Script with InfluxDB Support
# Set environment variables and flash to ESP32

# Source ESP-IDF environment (important for correct toolchain)
source ~/esp/esp-idf/export.sh

# Set LIBCLANG_PATH for bindgen
export LIBCLANG_PATH=/home/maulvin/.rustup/toolchains/esp/xtensa-esp32-elf-clang/esp-19.1.2_20250225/esp-clang/lib

echo "Building project in debug mode (more compatible)..."
LIBCLANG_PATH=$LIBCLANG_PATH cargo build || exit 1

echo "Flashing to ESP32..."
# Wait a moment for port to be available
sleep 2
echo "Trying espflash..."
if ! espflash flash --port /dev/ttyUSB0 --baud 115200 target/xtensa-esp32-espidf/debug/sht20; then
    echo "espflash failed, trying esptool.py with larger flash size..."
    sleep 2
    # Method 2: Use esptool.py with 16MB flash size
    esptool.py --chip esp32 --port /dev/ttyUSB0 --baud 115200 --before default_reset --after hard_reset write_flash --flash_size 16MB -z 0x1000 target/xtensa-esp32-espidf/debug/sht20
fi