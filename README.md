# SKT - Sistem Monitoring dan Kontrol Terintegrasi

Proyek ini adalah sistem monitoring dan kontrol industri terintegrasi yang menggabungkan pembacaan sensor SHT20, simulasi DWSIM, database InfluxDB, dan platform IoT ThingsBoard.

## 🏗️ Arsitektur Sistem

```
┌─────────────────┐    USB    ┌─────────────────┐    MQTT   ┌─────────────────┐
│   ESP32 + SHT20 │──Serial───│ Backend Service │───────────│   ThingsBoard   │
│   (Rust/ESP-IDF)│           │ (Serial Gateway │           │   (IoT Platform)│
└─────────────────┘           │ + InfluxDB      │           └─────────────────┘
                              │ + Bridge)       │
                              └─────────────────┘
                                       │
                                       │ HTTP API
                              ┌─────────────────┐
                              │    InfluxDB     │
                              │  (Time Series)  │
                              └─────────────────┘
                                       │
                              ┌─────────────────┐
                              │      DWSIM      │
                              │   (Python API)  │
                              └─────────────────┘
```

## 📁 Struktur Proyek

```
SKT/
├── sht20/                      # ESP32 Sensor Reader
│   ├── src/
│   │   └── main.rs            # Main aplikasi ESP32 (Serial output only)
│   └── Cargo.toml             # Dependencies Rust ESP32
├── backend/                   # Unified Backend Service
│   ├── src/
│   │   ├── main.rs           # Main service + InfluxDB bridge
│   │   └── serial.rs         # Serial gateway module
│   └── Cargo.toml            # Dependencies Rust
├── laporan/                   # Laporan Ilmiah LaTeX
│   ├── laporan.tex           # Main LaTeX document
│   ├── references.bib        # Bibliography (Harvard style)
│   ├── compile.sh            # Script kompilasi otomatis
│   └── images/               # Gambar untuk laporan
│       ├── hardware.jpeg     # Foto implementasi hardware
│       ├── Wiring Diagram.jpeg
│       ├── IOT.jpg
│       ├── SHT20 Parameter.png
│       ├── Backend Running.png
│       ├── Database Parameter.png
│       ├── Dwsim Simulation.png
│       ├── Dwsim Reading.png
│       ├── Dashboard Thingsboard.png
│       └── Latest Telemetry.png
├── dwsim.py                  # DWSIM Integration Script
├── data_recorder.py          # Script export data ThingsBoard
├── telemetry_data.csv        # Hasil pengujian sistem (5 jam)
└── README.md                 # Dokumentasi ini
```

## 🔧 Komponen Sistem

### 1. ESP32 SHT20 Sensor Reader (`sht20/`)

**Bahasa:** Rust dengan ESP-IDF framework
**Hardware:** ESP32, Sensor SHT20 via RS485, Motor Relay (GPIO2), Pump Relay (GPIO4)
**Fungsi:** Membaca data suhu dan kelembaban dari sensor SHT20, mengontrol relay motor dan pompa berdasarkan threshold, dan mengirimnya via serial USB

#### Metode Pembacaan Sensor:
- **Protokol:** Modbus RTU via UART (9600 baud)
- **Interface:** RS485 (GPIO16=TX, GPIO17=RX)
- **Slave Address:** 0x01
- **Function Code:** 0x04 (Read Input Registers)
- **Register Mapping:**
  - Temperature: Register 0x0001
  - Humidity: Register 0x0000 atau 0x0002 (fallback)

#### Alur Pembacaan:
1. **Inisialisasi UART** dengan konfigurasi 9600 baud, 8N1
2. **Kirim Request Suhu:**
   ```
   [01 04 00 01 00 01 CRC16_LO CRC16_HI]
   ```
3. **Baca Response Suhu:**
   ```
   [01 04 02 DATA_HI DATA_LO CRC16_LO CRC16_HI]
   ```
4. **Kirim Request Kelembaban:**
   ```
   [01 04 00 00 00 01 CRC16_LO CRC16_HI]
   ```
5. **Baca Response Kelembaban**
6. **Validasi CRC16** untuk memastikan integritas data
7. **Konversi nilai mentah** menjadi satuan fisik (°C, %)
8. **Terapkan offset kalibrasi** (Temperature: -1.2°C, Humidity: -6.5%)
9. **Kontrol relay otomatis** berdasarkan threshold suhu dan kelembaban
10. **Kirim status relay** via serial untuk monitoring

#### Fitur Khusus:
- **LED Indicator:** GPIO18 (TX), GPIO19 (RX) untuk status komunikasi
- **Relay Control:** GPIO2 (Motor), GPIO4 (Pump) dengan kontrol otomatis
- **Serial Output:** Format `SENSOR_DATA|timestamp|temperature|humidity` dan `RELAY_STATUS|motor:ON/OFF|pump:ON/OFF`
- **Automatic Control:** Motor ON saat suhu ≥30°C (OFF ≤25°C), Pump ON saat kelembaban ≤40% (OFF ≥60%)
- **Error Handling:** Robust error handling dengan detailed logging
- **Data Validation:** Range validation untuk data sensor
- **Offline Mode:** Tidak memerlukan WiFi, hanya output serial

### 2. InfluxDB Integration

**Database:** InfluxDB v2  
**Protokol:** HTTP API dengan Line Protocol  

#### Konfigurasi InfluxDB:
- **URL:** `http://192.168.121.64:8086`
- **Organization:** `ITS`
- **Bucket:** `SENSOR_DATA`
- **Authentication:** Token-based

#### Format Data (Line Protocol):
```
sht20_sensor temperature=25.30,humidity=65.20 1694168400000000000
```

#### Metode Upload:
1. **Generate timestamp** dalam nanoseconds (Unix epoch)
2. **Format Line Protocol** dengan measurement name dan fields
3. **HTTP POST** ke endpoint `/api/v2/write`
4. **Retry logic** dengan exponential backoff
5. **Status validation** (204 = success)

### 3. DWSIM Integration (`dwsim.py`)

**Bahasa:** Python 3  
**Framework:** pythonnet untuk .NET interop  
**Fungsi:** Mengambil data simulasi dari DWSIM dan upload ke InfluxDB

#### Dependensi:
```bash
pip install pythonnet influxdb-client
```

#### Metode Integrasi DWSIM:
1. **Load DWSIM Automation Library:**
   ```python
   clr.AddReference("DWSIM.Automation")
   from DWSIM.Automation import Automation2
   ```

2. **Connect to Running Simulation:**
   ```python
   automation = Automation2()
   simulations = automation.GetOpenedSimulations()
   ```

3. **Extract Stream Data:**
   ```python
   flowsheet_data = simulation.GetFlowsheetData()
   water_stream = find_stream_by_name("Water_i")
   ```

4. **Get Process Parameters:**
   - Temperature (°C)
   - Pressure (bar)
   - Mass flow (kg/s)
   - Density (kg/m³)
   - Enthalpy (kJ/kg)

#### Mode Operasi:
- **Single Run:** `python dwsim.py`
- **Continuous Monitoring:** `python dwsim.py continuous [interval]`

### 4. Unified Backend Service (`backend/`)

**Bahasa:** Rust dengan async/await
**Fungsi:**
- Serial gateway: Membaca data ESP32 via USB serial
- InfluxDB writer: Upload data sensor ke InfluxDB
- ThingsBoard bridge: Menjembatani data InfluxDB ke ThingsBoard via MQTT

#### Metode Bridging:
1. **Query InfluxDB** menggunakan Flux language:
   ```flux
   from(bucket: "SENSOR_DATA")
     |> range(start: -1h)
     |> filter(fn: (r) => r["_measurement"] == "sht20_sensor")
     |> aggregateWindow(every: 1m, fn: mean)
     |> last()
   ```

2. **Parse CSV Response** dari InfluxDB
3. **Format JSON Payload** untuk ThingsBoard:
   ```json
   {
     "sht20_temperature": 25.3,
     "sht20_humidity": 65.2,
     "dwsim_temperature": 85.5
   }
   ```

4. **MQTT Publish** ke ThingsBoard:
   - **Broker:** `mqtt.thingsboard.cloud:1883`
   - **Topic:** `v1/devices/me/telemetry`
   - **QoS:** At Least Once

#### Metode Serial Gateway:
1. **Monitor USB Serial** (`/dev/ttyUSB0` @ 115200 baud)
2. **Parse Format:** `SENSOR_DATA|timestamp|temperature|humidity`
3. **Upload to InfluxDB** menggunakan Line Protocol
4. **Auto-reconnect** jika serial terputus

#### Dependensi Rust:
```toml
[dependencies]
anyhow = "1.0"
reqwest = { version = "0.12", features = ["blocking", "json"] }
rumqttc = "0.24"
serde_json = "1.0"
serialport = "4.3"
tokio = { version = "1.0", features = ["full"] }
log = "0.4"
env_logger = "0.10"
```

### 5. ThingsBoard IoT Platform

**Platform:** ThingsBoard Cloud  
**Protokol:** MQTT  
**Fungsi:** Visualisasi dan monitoring data real-time

#### Konfigurasi:
- **Device Token:** `blcw1nufqg477ci07nlw`
- **Data Format:** JSON telemetry
- **Update Interval:** 10 detik

## 🚀 Cara Menjalankan

### 1. Setup ESP32 (SHT20 Sensor Reader)

```bash
# Masuk ke direktori sht20
cd sht20

# Build dan flash ke ESP32
cargo espflash flash --monitor

# Atau untuk development dengan auto-monitor
cargo espflash flash --monitor --port /dev/ttyUSB0
```

**Output ESP32:**
```
I (2345) sht20: SHT20 Data Logger - Serial Gateway Mode
I (2346) sht20: Serial output every 10 seconds
I (2347) sht20: LED: TX=GPIO18, RX=GPIO19
I (2348) sht20: Relay: Motor=GPIO2, Pump=GPIO4
I (2349) sht20: UART ready - RS485 9600 baud
I (12350) sht20: T: 25.3°C, H: 65.2%
I (12351) sht20: Motor: OFF, Pump: ON (T<30°C, H<60%)
SENSOR_DATA|1694168400000000000|25.30|65.20
RELAY_STATUS|motor:OFF|pump:ON
INFLUX_LINE|sht20_sensor temperature=25.30,humidity=65.20,motor_status=0,pump_status=1 1694168400000000000
```

### 2. Setup InfluxDB

```bash
# Install InfluxDB v2
# Buat organization "ITS"
# Buat bucket "SENSOR_DATA" dan "DWSIM_DATA"
# Generate API token dan update di konfigurasi
```

### 3. Jalankan DWSIM Integration (Opsional)

```bash
# Install dependensi Python
pip install pythonnet influxdb-client

# Jalankan DWSIM dan buka simulasi
# Pastikan ada stream bernama "Water_i"

# Jalankan script
python dwsim.py continuous 10
```

### 4. Jalankan Unified Backend Service

```bash
# Masuk ke direktori backend
cd backend

# Jalankan dengan logging
RUST_LOG=info cargo run

# Output yang diharapkan:
# [INFO] 🚀 Backend started:
# [INFO]   - Serial monitoring: /dev/ttyUSB0 @ 115200 baud
# [INFO]   - InfluxDB bridge: http://localhost:8086 → ThingsBoard
# [INFO]   - Query interval: 10 seconds
# [INFO] ✓ MQTT connected to ThingsBoard
# [INFO] ESP32: SENSOR_DATA|1694168400000000000|25.30|65.20
# [INFO] ESP32: RELAY_STATUS|motor:OFF|pump:ON
# [INFO] Sensor data uploaded: T=25.3°C, H=65.2%, Motor=OFF, Pump=ON
```

**Fitur Backend:**
- ✅ **Serial Gateway**: Auto-detect ESP32 pada `/dev/ttyUSB0`
- ✅ **InfluxDB Upload**: Data sensor langsung ke database
- ✅ **ThingsBoard Bridge**: Query InfluxDB → publish MQTT
- ✅ **Auto-reconnect**: Serial dan MQTT auto-reconnect
- ✅ **Logging**: Structured logging dengan timestamps

### 5. Setup ThingsBoard

1. Daftar di ThingsBoard Cloud
2. Buat device baru
3. Copy device token ke konfigurasi bridge
4. Buat dashboard untuk visualisasi data

## 📊 Export Data dari ThingsBoard

Script `data_recorder.py` digunakan untuk export telemetry data dari ThingsBoard ke file CSV.

### Setup Data Recorder

1. **Install dependencies:**
```bash
pip install requests python-dotenv
```

2. **Buat file `.env` di root project:**
```bash
THINGSBOARD_HOST=demo.thingsboard.io
THINGSBOARD_PORT=80
THINGSBOARD_USERNAME=your_email@example.com
THINGSBOARD_PASSWORD=your_password
DEVICE_ID=your_device_id
TELEMETRY_KEYS=sht20_temperature,sht20_humidity,motor_status,pump_status,dwsim_temperature
```

3. **Jalankan script:**
```bash
python data_recorder.py
```

### Fitur Data Recorder:
- ✅ **Authentication**: JWT token-based login ke ThingsBoard
- ✅ **Time Range**: Default 24 jam terakhir (customizable)
- ✅ **Multiple Keys**: Support multiple telemetry keys sekaligus
- ✅ **CSV Format**: Format lebar (wide format) dengan timestamp
- ✅ **Error Handling**: Robust error handling dan logging
- ✅ **Large Data**: Support hingga 10,000 data points per query

### Output Format CSV:
```csv
timestamp,sht20_temperature,sht20_humidity,motor_status,pump_status,dwsim_temperature
2025-11-06T04:19:50.567000,30.74,68.78,1,0,25.0
2025-11-06T04:19:55.667000,30.77,68.71,1,0,25.0
...
```

### Kustomisasi Time Range:
Edit variabel di `data_recorder.py`:
```python
# Untuk 5 jam terakhir
START_TS = END_TS - (5 * 60 * 60 * 1000)

# Untuk 7 hari terakhir
START_TS = END_TS - (7 * 24 * 60 * 60 * 1000)
```

## 📊 Data Flow

1. **ESP32** membaca sensor SHT20 setiap 10 detik
2. **ESP32** kontrol relay berdasarkan threshold (Motor: T≥30°C, Pump: H≤40%)
3. **ESP32** kirim data via USB serial: `SENSOR_DATA|timestamp|temp|humidity` dan `RELAY_STATUS|motor:ON/OFF|pump:ON/OFF`
4. **Backend** parsing serial data dan upload ke InfluxDB bucket `SENSOR_DATA` (termasuk status relay)
5. **Python script** (opsional) mengambil data DWSIM dan upload ke InfluxDB bucket `DWSIM_DATA`
6. **Backend** query data dari InfluxDB setiap 10 detik
7. **Data terbaru** (sensor + relay + DWSIM) dipublish ke ThingsBoard via MQTT
8. **ThingsBoard** menampilkan data real-time di dashboard

**Keuntungan arsitektur ini:**
- ✅ ESP32 tidak perlu WiFi (lebih stabil)
- ✅ Satu backend service untuk semua
- ✅ Data tetap masuk InfluxDB dan ThingsBoard
- ✅ Monitoring dan logging terpusat

## ⚙️ Konfigurasi

### ESP32 (Tidak ada konfigurasi khusus)
ESP32 hanya output serial, tidak perlu WiFi atau konfigurasi network.

### Backend Service
```rust
// backend/src/main.rs
const INFLUX_URL: &str = "http://localhost:8086";
const ORG: &str = "ITS";
const TOKEN: &str = "your-influxdb-token";
const SENSOR_BUCKET: &str = "SENSOR_DATA";
const DWSIM_BUCKET: &str = "DWSIM_DATA";

// Serial port configuration
const SERIAL_PORT: &str = "/dev/ttyUSB0";
const BAUD_RATE: u32 = 115200;
```

### ThingsBoard
```rust
// backend/src/main.rs
const TB_HOST: &str = "mqtt.thingsboard.cloud";
const TB_TOKEN: &str = "your-device-token";
```

## 🔍 Troubleshooting

### ESP32 Issues:
- **Flash gagal:** Tekan tombol BOOT saat flashing, coba port USB lain
- **Sensor tidak terbaca:** Periksa koneksi RS485 dan wiring (GPIO16/17)
- **No serial output:** Periksa koneksi USB dan baud rate
- **Relay tidak berfungsi:** Periksa koneksi GPIO2 (motor) dan GPIO4 (pump)
- **LED tidak menyala:** Periksa koneksi GPIO18 (TX) dan GPIO19 (RX)

### DWSIM Issues:
- **Library tidak ditemukan:** Install `pythonnet` dan pastikan DWSIM terinstall
- **Simulasi tidak ditemukan:** Pastikan DWSIM running dengan simulasi terbuka
- **Stream tidak ditemukan:** Periksa nama stream di DWSIM (harus "Water_i")

### Backend Issues:
- **Serial tidak terbaca:** Pastikan ESP32 terhubung di `/dev/ttyUSB0`
- **Serial port busy:** Gunakan `lsof /dev/ttyUSB0` dan `kill` untuk menghentikan proses lain
- **InfluxDB upload gagal:** Periksa InfluxDB service dan token
- **MQTT connection gagal:** Periksa ThingsBoard device token
- **No data found:** Pastikan ada data di InfluxDB dalam range waktu yang ditentukan
- **Serial timeout:** Backend menggunakan timeout 15s untuk menunggu data ESP32 (interval 10s)
- **Async/blocking mix panic:** Pastikan menggunakan async reqwest client, bukan blocking

### Quick Debug Commands:
```bash
# Cek serial port tersedia
ls -la /dev/ttyUSB*

# Monitor serial langsung
sudo screen /dev/ttyUSB0 115200

# Test InfluxDB connection
curl -H "Authorization: Token YOUR_TOKEN" \
     "http://localhost:8086/api/v2/buckets?org=ITS"
```

## 📈 Monitoring & Logs

Semua komponen menggunakan structured logging:
- **ESP32:** Serial output via USB (dapat dimonitor langsung)
- **Backend:** env_logger dengan level INFO/DEBUG/ERROR
- **Python:** Console output dengan timestamps
- **InfluxDB:** Built-in monitoring UI di port 8086
- **ThingsBoard:** Device telemetry dan logs

### Log Levels:
```bash
# Info level (production)
RUST_LOG=info cargo run

# Debug level (development)
RUST_LOG=debug cargo run

# Specific module debugging
RUST_LOG=backend::serial=debug cargo run
```

## 🛠️ Development

### Build Requirements:
- **Rust:** 1.77+ dengan ESP-IDF toolchain
- **Python:** 3.8+ dengan pythonnet
- **InfluxDB:** v2.x
- **ThingsBoard:** Cloud atau self-hosted

### Hardware Requirements:
- **ESP32** development board
- **SHT20** sensor dengan RS485 interface
- **RS485 to TTL** converter
- **USB cable** untuk koneksi serial ESP32 ↔ Computer
- **Motor relay module** untuk GPIO2 (opsional)
- **Pump relay module** untuk GPIO4 (opsional)
- **LED indicators** untuk GPIO18 dan GPIO19 (opsional)

## 📄 Laporan Ilmiah

Laporan lengkap penelitian tersedia dalam format PDF di folder `laporan/`.

### Kompilasi Laporan

Laporan ditulis menggunakan LaTeX dengan format Harvard citation style.

#### Cara 1: Otomatis (Recommended)
```bash
cd laporan
./compile.sh
```

#### Cara 2: Manual
```bash
cd laporan
pdflatex laporan.tex
bibtex laporan
pdflatex laporan.tex
pdflatex laporan.tex
```

#### Spesifikasi Laporan:
- **Format**: A4, 1.15 line spacing
- **Citation**: Harvard (author-year) style
- **Sections**: Abstract, 5 BAB, Daftar Pustaka, Lampiran
- **Pages**: 35 halaman
- **Bibliography**: 23 referensi terverifikasi

### Data Pengujian

File `telemetry_data.csv` berisi hasil pengujian sistem selama 5 jam:
- **Total data points**: 3,173
- **Interval**: ~5.7 detik
- **Durasi**: 04:19:50 - 08:50:44 WIB (6 November 2025)
- **Success rate**: 100% (0 data loss)

**Statistik Pengujian:**
- Temperature: 30.69°C - 30.83°C (avg 30.75°C)
- Humidity: 66.49% - 69.90% (avg 68.52%)
- Motor status: ON 100% waktu (temp > 30°C)
- Pump status: OFF 100% waktu (humidity > 60%)


**gnuplot**
set datafile separator ","
set xdata time
set timefmt "%Y-%m-%dT%H:%M:%S"
set format x "%H:%M"

set xlabel "Time"
set ylabel "Temperature (°C)" textcolor rgb "red"
set y2label "Humidity (%)" textcolor rgb "blue"
set y2tics

set title "Temperature and Humidity Data"

plot 'telemetry_data.csv' using 1:2 with lines title "SHT20 Temperature" axes x1y1 lc rgb "red", \
     '' using 1:3 with lines title "SHT20 Humidity" axes x1y2 lc rgb "blue"

## 📄 Lisensi


---

**Developed by:** SKT Team  
**Version:** 1.1.0  
**Last Updated:** November 2025
