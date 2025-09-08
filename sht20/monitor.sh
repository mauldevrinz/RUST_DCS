#!/bin/bash
# ESP32 Serial Monitor Script for Ubuntu

echo "🔌 Connecting to ESP32 on /dev/ttyUSB0..."
echo "📝 Log will be saved to esp32.log"
echo "🔄 Press Ctrl+A then K then Y to exit"
echo ""

# Start screen session with logging
screen -L -Logfile esp32.log -S esp32 /dev/ttyUSB0 115200