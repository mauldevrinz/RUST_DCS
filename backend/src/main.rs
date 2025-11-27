use anyhow::{anyhow, Result};
use reqwest::Client;
use rumqttc::{Client as MqttClient, Event, Incoming, MqttOptions, QoS};
use serde_json::json;
use std::{thread, time::Duration};
use log::{info, error};

mod serial;
use serial::{SerialMonitor, SensorData};

// ===================== KONFIGURASI ANDA =====================
const INFLUX_URL: &str = "http://localhost:8086";
const ORG:        &str = "ITS";
const TOKEN:      &str = "pFlhPKsrTfaJ6-iIKz46wwHuKPOkp8GBK_chLeWCxpTgeFryMn9feiUukWZe5DAm4ocDJUAlPlyBaw8zg9PDYQ==";

// Data dari sensor SHT20
const SENSOR_BUCKET: &str = "SENSOR_DATA";
const SENSOR_MEAS:   &str = "sht20_sensor";

// Data dari DWSIM
const DWSIM_BUCKET: &str = "DWSIM_DATA";
const DWSIM_MEAS:   &str = "dwsim_temperature";

// ThingsBoard
const TB_HOST:  &str = "demo.thingsboard.io";
const TB_PORT:  u16 = 1883;
const TB_TOKEN: &str = "8h0YBHyEU8dUvJ4PdYL9";

// Rentang waktu & window untuk query InfluxDB
const RANGE:  &str = "-1h";
const WINDOW: &str = "1m";
// Serial port configuration
const SERIAL_PORT: &str = "/dev/ttyUSB0";
const BAUD_RATE: u32 = 115200;
// ==========================================================

// Helper function to write data to InfluxDB
async fn write_sensor_to_influx(client: &Client, data: &SensorData) -> Result<()> {
    let mut line = format!(
        "sht20_sensor temperature={:.2},humidity={:.2}",
        data.temperature, data.humidity
    );

    // Only save pump_status, NOT exhaust_fan_status (will be calculated virtually by backend)
    if let Some(pump) = data.pump_status {
        line.push_str(&format!(",pump_status={}", if pump { 1 } else { 0 }));
    }

    let timestamp_ns = if data.timestamp < 1_000_000_000_000_000_000 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    } else {
        data.timestamp
    };

    line.push_str(&format!(" {}", timestamp_ns));

    let url = format!("{}/api/v2/write", INFLUX_URL);

    let response = client
        .post(&url)
        .header("Authorization", format!("Token {}", TOKEN.trim()))
        .header("Content-Type", "text/plain")
        .query(&[("org", ORG), ("bucket", SENSOR_BUCKET)])
        .body(line)
        .send()
        .await?;

    if response.status().is_success() {
        let pump_str = data.pump_status.map(|p| if p { "ON" } else { "OFF" }).unwrap_or("N/A");
        info!("Data uploaded: T={:.1}°C, H={:.1}%, Pump={}", 
              data.temperature, data.humidity, pump_str);
    } else {
        error!("InfluxDB upload failed: {}", response.status());
    }

    Ok(())
}

// Write calculated exhaust fan status to InfluxDB
async fn write_fan_status_to_influx(client: &Client, fan_on: i32, sensor_temp: f64, setpoint_temp: f64) -> Result<()> {
    let line = format!(
        "sht20_sensor exhaust_fan_status={},sensor_temp={:.2},setpoint_temp={:.2} {}",
        fan_on,
        sensor_temp,
        setpoint_temp,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    );

    let url = format!("{}/api/v2/write", INFLUX_URL);

    let response = client
        .post(&url)
        .header("Authorization", format!("Token {}", TOKEN.trim()))
        .header("Content-Type", "text/plain")
        .query(&[("org", ORG), ("bucket", SENSOR_BUCKET)])
        .body(line)
        .send()
        .await?;

    if response.status().is_success() {
        info!("Fan status saved to InfluxDB: {}", if fan_on == 1 { "ON" } else { "OFF" });
    } else {
        error!("InfluxDB fan status write failed: {}", response.status());
    }

    Ok(())
}

// Write calculated pump status to InfluxDB based on humidity
async fn write_pump_status_to_influx(client: &Client, pump_on: i32, humidity: f64) -> Result<()> {
    let line = format!(
        "sht20_sensor pump_calculated_status={},humidity={:.2} {}",
        pump_on,
        humidity,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    );

    let url = format!("{}/api/v2/write", INFLUX_URL);

    let response = client
        .post(&url)
        .header("Authorization", format!("Token {}", TOKEN.trim()))
        .header("Content-Type", "text/plain")
        .query(&[("org", ORG), ("bucket", SENSOR_BUCKET)])
        .body(line)
        .send()
        .await?;

    if response.status().is_success() {
        info!("💧 Pump status saved to InfluxDB: {} (Humidity: {:.1}%)", if pump_on == 1 { "ON" } else { "OFF" }, humidity);
    } else {
        error!("InfluxDB pump status write failed: {}", response.status());
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let http = Client::new();

    // MQTT ThingsBoard
    let mut mqtt = MqttOptions::new("rust-bridge", TB_HOST, TB_PORT);
    mqtt.set_credentials(TB_TOKEN, "");
    mqtt.set_keep_alive(Duration::from_secs(30));

    let (cli, mut conn) = MqttClient::new(mqtt, 10);
    thread::spawn(move || {
        for ev in conn.iter() {
            match ev {
                Ok(Event::Incoming(Incoming::ConnAck(_))) => info!("✓ MQTT connected to ThingsBoard"),
                Ok(Event::Incoming(Incoming::PingResp)) => {} // Do nothing for PingResp
                Err(e) => error!("MQTT event error: {e:#}"),
                _ => {} // Ignore other events
            }
        }
    });

    // Start serial monitoring in background
    let http_for_serial = http.clone();
    let serial_monitor = SerialMonitor::new(SERIAL_PORT.to_string(), BAUD_RATE);

    tokio::spawn(async move {
        if let Err(e) = serial_monitor.start_monitoring(move |data| {
            let http_clone = http_for_serial.clone();
            tokio::spawn(async move {
                if let Err(e) = write_sensor_to_influx(&http_clone, &data).await {
                    error!("Failed to upload sensor data: {}", e);
                }
            });
            Ok(())
        }).await {
            error!("Serial monitoring failed: {}", e);
        }
    });

    info!("🚀 Backend started:");
    info!("  - Serial monitoring: {} @ {} baud", SERIAL_PORT, BAUD_RATE);
    info!("  - DWSIM setpoint control enabled");
    info!("  - InfluxDB bridge: {} → ThingsBoard", INFLUX_URL);
    info!("  - Query interval: {} seconds", 10);

    loop {
        info!("Querying InfluxDB for bridge data...");
        let sensor_data = get_last_data(&http, SENSOR_BUCKET, SENSOR_MEAS, RANGE, WINDOW).await?;
        let dwsim_data = get_dwsim_temperature(&http, DWSIM_BUCKET, DWSIM_MEAS, RANGE, WINDOW).await?;

        let mut payload = serde_json::Map::new();
        if let Some(t) = sensor_data.temp { payload.insert("sht20_temperature".into(), json!(t)); }
        if let Some(h) = sensor_data.hum  { payload.insert("sht20_humidity".into(), json!(h)); }
        if let Some(p) = sensor_data.pump_status { payload.insert("pump_status".into(), json!(p as i32)); }
        if let Some(t) = dwsim_data.temp  { payload.insert("dwsim_temperature".into(), json!(t)); }

        // Hitung exhaust_fan_status berdasarkan DWSIM setpoint
        if let (Some(sensor_temp), Some(setpoint_temp)) = (sensor_data.temp, dwsim_data.temp) {
            // Fan ON jika sensor_temp > setpoint_temp
            let fan_on = if sensor_temp > setpoint_temp { 1 } else { 0 };
            payload.insert("exhaust_fan_status".into(), json!(fan_on));
            
            info!("🔥 Fan Status: Sensor={:.2}°C, Setpoint={:.2}°C → Fan={}", 
                  sensor_temp, setpoint_temp, if fan_on == 1 { "ON" } else { "OFF" });
            payload.insert("dwsim_temperature_setpoint".into(), json!(setpoint_temp));
            
            // Simpan fan status yang sudah dihitung ke InfluxDB
            if let Err(e) = write_fan_status_to_influx(&http, fan_on, sensor_temp, setpoint_temp).await {
                error!("Failed to write fan status to InfluxDB: {}", e);
            }
        }

        // Hitung pump_status berdasarkan humidity (ON jika < 60%, OFF jika >= 60%)
        if let Some(humidity) = sensor_data.hum {
            let pump_on = if humidity < 60.0 { 1 } else { 0 };
            payload.insert("pump_calculated_status".into(), json!(pump_on));
            
            info!("💧 Pump Status: Humidity={:.1}% → Pump={}", 
                  humidity, if pump_on == 1 { "ON" } else { "OFF" });
            
            // Simpan pump status yang sudah dihitung ke InfluxDB
            if let Err(e) = write_pump_status_to_influx(&http, pump_on, humidity).await {
                error!("Failed to write pump status to InfluxDB: {}", e);
            }
        }

        if payload.is_empty() {
            error!("⚠️  No data from InfluxDB (check range/window/measurement/tag/field).");
        } else {
            let body = json!(payload).to_string();
            info!("→ Publishing to ThingsBoard: {}", body);
            if let Err(e) = cli.publish("v1/devices/me/telemetry", QoS::AtLeastOnce, false, body) {
                error!("MQTT publish error: {e:#}");
            }
        }

        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

#[derive(Default, Debug, Clone, Copy)]
struct LastRow {
    temp: Option<f64>,
    hum: Option<f64>,
    exhaust_fan_status: Option<f64>,
    pump_status: Option<f64>,
}

#[derive(Default, Debug, Clone, Copy)]
struct DwsimRow { temp: Option<f64> }

// Fungsi untuk mengirim query ke InfluxDB
async fn post_influx(client: &Client, flux: String) -> Result<String> {
    let url = format!("{INFLUX_URL}/api/v2/query?org={ORG}");
    let resp = client
        .post(&url)
        .header("Authorization", format!("Token {}", TOKEN.trim()))
        .header("Accept", "application/csv")
        .header("Content-Type", "application/vnd.flux")
        .body(flux)
        .send()
        .await?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("Influx query FAILED: {} | {}", status, body.trim()));
    }
    log::debug!("--- InfluxDB CSV Response ---\n{}\n-----------------------------", body.trim());
    Ok(body)
}

// Mengambil data terakhir menggunakan metode aggregateWindow (cara yang benar)
async fn get_last_data(
    client: &Client,
    bucket: &str,
    measurement: &str,
    range: &str,
    window: &str,
) -> Result<LastRow> {
    // PENTING: Sesuaikan nama field di sini jika berbeda dengan "temperature" & "humidity"
    // PENTING: Sesuaikan nama field di sini jika berbeda dengan "temperature" & "humidity"
    let flux = format!(r#"from(bucket: "{bucket}")
  |> range(start: {range})
  |> filter(fn: (r) => r["_measurement"] == "{measurement}")
  |> filter(fn: (r) => r["_field"] == "temperature" or r["_field"] == "humidity" or r["_field"] == "exhaust_fan_status" or r["_field"] == "pump_status")
  |> aggregateWindow(every: {window}, fn: mean, createEmpty: false)
  |> group(columns: ["_field"])
  |> last()
"#);

    let csv = post_influx(client, flux).await?;
    Ok(parse_influx_csv(&csv))
}

// Mengambil data temperature dari DWSIM_DATA bucket untuk Water_i stream
async fn get_dwsim_temperature(
    client: &Client,
    bucket: &str,
    measurement: &str,
    range: &str,
    window: &str,
) -> Result<DwsimRow> {
    // Query debug: coba lihat semua data di bucket terlebih dahulu
    let debug_flux = format!(r#"from(bucket: "{bucket}")
  |> range(start: -24h)
  |> filter(fn: (r) => r["_measurement"] == "{measurement}")
  |> limit(n: 5)
"#);
    
    log::debug!("🔍 Debug: Checking DWSIM bucket contents for measurement '{measurement}'...");
    if let Ok(debug_csv) = post_influx(client, debug_flux).await {
        if debug_csv.trim().is_empty() || debug_csv.lines().count() <= 1 {
            log::warn!("⚠️  DWSIM bucket '{bucket}' has no data for measurement '{measurement}' in last 24h");

            // Try to see what measurements exist
            let all_meas_flux = format!(r#"from(bucket: "{bucket}")
  |> range(start: -24h)
  |> group(columns: ["_measurement"])
  |> distinct(column: "_measurement")
  |> limit(n: 10)
"#);
            if let Ok(meas_csv) = post_influx(client, all_meas_flux).await {
                log::debug!("Available measurements in bucket:");
                for line in meas_csv.lines() {
                    if !line.starts_with('#') && !line.contains("_measurement") {
                        log::debug!("  - {}", line);
                    }
                }
            }
        } else {
            log::debug!("✓ Found data in DWSIM bucket for measurement '{measurement}'");
        }
    }

    // Filter for Water_i stream specifically to get the actual simulated temperature
    let flux = format!(r#"from(bucket: "{bucket}")
  |> range(start: {range})
  |> filter(fn: (r) => r["_measurement"] == "{measurement}")
  |> filter(fn: (r) => r["stream"] == "Water_i")
  |> filter(fn: (r) => r["_field"] == "temperature_celsius")
  |> aggregateWindow(every: {window}, fn: mean, createEmpty: false)
  |> last()
"#);

    let csv = post_influx(client, flux).await?;
    Ok(parse_dwsim_csv(&csv))
}

// Parser CSV yang sesuai dengan hasil query aggregateWindow
fn parse_influx_csv(csv: &str) -> LastRow {
    let mut idx_field: Option<usize> = None;
    let mut idx_value: Option<usize> = None;
    let mut header_seen = false;
    let mut out = LastRow::default();

    for line in csv.lines() {
        if line.starts_with('#') { continue; }
        let cols: Vec<&str> = line.split(',').collect();

        if !header_seen && (cols.contains(&"_field") || cols.contains(&"_value")) {
            for (i, c) in cols.iter().enumerate() {
                if *c == "_field" { idx_field = Some(i); }
                if *c == "_value" { idx_value = Some(i); }
            }
            header_seen = true;
            continue;
        }

        if header_seen {
            if let (Some(i_f), Some(i_v)) = (idx_field, idx_value) {
                if i_f < cols.len() && i_v < cols.len() {
                    let fname = cols[i_f].trim();
                    let val = cols[i_v].trim().parse::<f64>().ok();
                    match (fname, val) {
                        ("temperature", Some(v)) => out.temp = Some(v),
                        ("humidity",    Some(v)) => out.hum  = Some(v),
                        ("exhaust_fan_status", Some(v)) => out.exhaust_fan_status = Some(v),
                        ("pump_status", Some(v)) => out.pump_status = Some(v),
                        _ => {} // Ignore other fields
                    }
                }
            }
        }
    }
    out
}

// Parser CSV untuk DWSIM temperature data
fn parse_dwsim_csv(csv: &str) -> DwsimRow {
    let mut idx_field: Option<usize> = None;
    let mut idx_value: Option<usize> = None;
    let mut header_seen = false;
    let mut out = DwsimRow::default();

    for line in csv.lines() {
        if line.starts_with('#') { continue; }
        let cols: Vec<&str> = line.split(',').collect();

        if !header_seen && (cols.contains(&"_field") || cols.contains(&"_value")) {
            for (i, c) in cols.iter().enumerate() {
                if *c == "_field" { idx_field = Some(i); }
                if *c == "_value" { idx_value = Some(i); }
            }
            header_seen = true;
            continue;
        }

        if header_seen {
            if let (Some(i_f), Some(i_v)) = (idx_field, idx_value) {
                if i_f < cols.len() && i_v < cols.len() {
                    let fname = cols[i_f].trim();
                    let val = cols[i_v].trim().parse::<f64>().ok();
                    if fname == "temperature_celsius" {
                        if let Some(v) = val {
                            out.temp = Some(v);
                        }
                    }
                }
            }
        }
    }
    out
}