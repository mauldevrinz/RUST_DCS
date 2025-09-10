# SKT - Sistem Monitoring dan Kontrol Terintegrasi

Proyek ini adalah sistem monitoring dan kontrol industri terintegrasi yang menggabungkan pembacaan sensor SHT20, simulasi DWSIM, database InfluxDB, dan platform IoT ThingsBoard.

## 🏗️ Arsitektur Sistem

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   ESP32 + SHT20 │────│    InfluxDB     │────│   ThingsBoard   │
│   (Rust/ESP-IDF)│    │  (Time Series)  │    │   (IoT Platform)│
└─────────────────┘    └─────────────────┘    └─────────────────┘
         │                       │                       │
         │              ┌─────────────────┐              │
         │              │      DWSIM      │              │
         └──────────────│   (Python API)  │──────────────┘
                        └─────────────────┘
                               │
                    ┌─────────────────┐
                    │ Bridge Service  │
                    │    (Rust)       │
                    └─────────────────┘
```

## 📁 Struktur Proyek

```
SKT/
├── sht20/                      # ESP32 Sensor Reader
│   ├── src/
│   │   ├── main.rs            # Main aplikasi ESP32
│   │   └── config.rs          # Konfigurasi WiFi & InfluxDB
│   └── Cargo.toml             # Dependencies Rust ESP32
├── backend/                   # InfluxDB-ThingsBoard Bridge
│   ├── src/
│   │   └── main.rs           # Bridge service
│   └── Cargo.toml            # Dependencies Rust
├── dwsim.py                  # DWSIM Integration Script
└── README.md                 # Dokumentasi ini
```

## 🔧 Komponen Sistem

### 1. ESP32 SHT20 Sensor Reader (`sht20/`)

**Bahasa:** Rust dengan ESP-IDF framework  
**Hardware:** ESP32, Sensor SHT20 via RS485  
**Fungsi:** Membaca data suhu dan kelembaban dari sensor SHT20 dan mengirimnya ke InfluxDB

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

#### Fitur Khusus:
- **LED Indicator:** GPIO2 (TX), GPIO4 (RX) untuk status komunikasi
- **WiFi Connection:** Auto-connect dengan retry mechanism
- **Time Sync:** SNTP untuk timestamp presisi
- **Error Handling:** Robust error handling dengan detailed logging
- **Data Validation:** Range validation untuk data sensor

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

### 4. InfluxDB-ThingsBoard Bridge (`backend/`)

**Bahasa:** Rust  
**Fungsi:** Menjembatani data dari InfluxDB ke ThingsBoard via MQTT

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

#### Dependensi Rust:
```toml
[dependencies]
anyhow = "1.0"
reqwest = { version = "0.12", features = ["blocking", "json"] }
rumqttc = "0.24"
serde_json = "1.0"
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

# Update konfigurasi di src/config.rs
# - WIFI_SSID dan WIFI_PASSWORD
# - INFLUXDB_URL, INFLUXDB_ORG, INFLUXDB_BUCKET, INFLUXDB_TOKEN

# Build dan flash ke ESP32
cargo build
cargo run
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

### 4. Jalankan Bridge Service

```bash
# Masuk ke direktori backend
cd backend

# Jalankan bridge
cargo run
```

### 5. Setup ThingsBoard

1. Daftar di ThingsBoard Cloud
2. Buat device baru
3. Copy device token ke konfigurasi bridge
4. Buat dashboard untuk visualisasi data

## 📊 Data Flow

1. **ESP32** membaca sensor SHT20 setiap 10 detik
2. **Data sensor** dikirim ke InfluxDB bucket `SENSOR_DATA`
3. **Python script** (opsional) mengambil data DWSIM dan upload ke InfluxDB
4. **Bridge service** query data dari InfluxDB setiap 10 detik
5. **Data terbaru** dipublish ke ThingsBoard via MQTT
6. **ThingsBoard** menampilkan data real-time di dashboard

## ⚙️ Konfigurasi

### WiFi & Network
```rust
// sht20/src/config.rs
pub const WIFI_SSID: &str = "YourWiFiSSID";
pub const WIFI_PASSWORD: &str = "YourWiFiPassword";
```

### InfluxDB
```rust
// Semua file konfigurasi
pub const INFLUXDB_URL: &str = "http://192.168.121.64:8086";
pub const INFLUXDB_ORG: &str = "ITS";
pub const INFLUXDB_BUCKET: &str = "SENSOR_DATA";
pub const INFLUXDB_TOKEN: &str = "your-token-here";
```

### ThingsBoard
```rust
// backend/src/main.rs
const TB_HOST: &str = "mqtt.thingsboard.cloud";
const TB_TOKEN: &str = "your-device-token";
```

## 🔍 Troubleshooting

### ESP32 Issues:
- **WiFi tidak connect:** Periksa SSID/password di `config.rs`
- **Sensor tidak terbaca:** Periksa koneksi RS485 dan wiring
- **InfluxDB upload gagal:** Periksa network connectivity dan token

### DWSIM Issues:
- **Library tidak ditemukan:** Install `pythonnet` dan pastikan DWSIM terinstall
- **Simulasi tidak ditemukan:** Pastikan DWSIM running dengan simulasi terbuka
- **Stream tidak ditemukan:** Periksa nama stream di DWSIM (harus "Water_i")

### Bridge Issues:
- **InfluxDB query gagal:** Periksa bucket name dan data availability
- **MQTT connection gagal:** Periksa ThingsBoard device token
- **No data found:** Pastikan ada data di InfluxDB dalam range waktu yang ditentukan

## 📈 Monitoring & Logs

Semua komponen menggunakan structured logging:
- **ESP32:** Serial output via USB
- **Python:** Console output dengan timestamps
- **Bridge:** Console output dengan status indicators
- **InfluxDB:** Built-in monitoring UI
- **ThingsBoard:** Device telemetry dan logs

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
- Stable WiFi connection

## 📄 Lisensi

MIT License - lihat file LICENSE untuk detail lengkap.

---

**Developed by:** SKT Team  
**Version:** 1.0.0  
**Last Updated:** September 2025