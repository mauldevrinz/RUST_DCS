use anyhow::{anyhow, Result};
use reqwest::blocking::Client;
use rumqttc::{Client as MqttClient, Event, Incoming, MqttOptions, QoS};
use serde_json::json;
use std::{thread, time::Duration};

// ===================== KONFIGURASI ANDA =====================
const INFLUX_URL: &str = "http://192.168.121.64:8086";
const ORG:        &str = "ITS";
const TOKEN:      &str = "pFlhPKsrTfaJ6-iIKz46wwHuKPOkp8GBK_chLeWCxpTgeFryMn9feiUukWZe5DAm4ocDJUAlPlyBaw8zg9PDYQ==";

// Data dari sensor SHT20
const SENSOR_BUCKET: &str = "SENSOR_DATA";
const SENSOR_MEAS:   &str = "sht20_sensor";

// Data dari DWSIM
const DWSIM_BUCKET: &str = "DWSIM_DATA";
const DWSIM_MEAS:   &str = "temperature";

// ThingsBoard
const TB_HOST:  &str = "mqtt.thingsboard.cloud";
const TB_PORT:  u16 = 1883;
const TB_TOKEN: &str = "blcw1nufqg477ci07nlw";

// Rentang waktu & window untuk query InfluxDB
const RANGE:  &str = "-1h";
const WINDOW: &str = "1m";
// ==========================================================

fn main() -> Result<()> {
    let http = Client::builder().danger_accept_invalid_certs(true).build()?;

    // MQTT ThingsBoard
    let mut mqtt = MqttOptions::new("rust-bridge", TB_HOST, TB_PORT);
    mqtt.set_credentials(TB_TOKEN, "");
    mqtt.set_keep_alive(Duration::from_secs(30));

    let (cli, mut conn) = MqttClient::new(mqtt, 10);
    thread::spawn(move || {
        for ev in conn.iter() {
            match ev {
                Ok(Event::Incoming(Incoming::ConnAck(_))) => println!("✓ MQTT connected to ThingsBoard"),
                Ok(Event::Incoming(Incoming::PingResp)) => {} // Do nothing for PingResp
                Err(e) => eprintln!("MQTT event error: {e:#}"),
                _ => {} // Ignore other events
            }
        }
    });

    loop {
        println!("\nQuering InfluxDB...");
        let sensor_data = get_last_data(&http, SENSOR_BUCKET, SENSOR_MEAS, RANGE, WINDOW)?;
        let dwsim_data = get_dwsim_temperature(&http, DWSIM_BUCKET, DWSIM_MEAS, RANGE, WINDOW)?;

        let mut payload = serde_json::Map::new();
        if let Some(t) = sensor_data.temp { payload.insert("sht20_temperature".into(), json!(t)); }
        if let Some(h) = sensor_data.hum  { payload.insert("sht20_humidity".into(), json!(h)); }
        if let Some(t) = dwsim_data.temp  { payload.insert("dwsim_temperature".into(), json!(t)); }

        if payload.is_empty() {
            eprintln!("⚠️  Tidak ada data dari Influx (cek range/window/measurement/tag/field).");
        } else {
            let body = json!(payload).to_string();
            println!("→ publish TB: {body}");
            if let Err(e) = cli.publish("v1/devices/me/telemetry", QoS::AtLeastOnce, false, body) {
                eprintln!("MQTT publish error: {e:#}");
            }
        }

        thread::sleep(Duration::from_secs(10));
    }
}

#[derive(Default, Debug, Clone, Copy)]
struct LastRow { temp: Option<f64>, hum: Option<f64> }

#[derive(Default, Debug, Clone, Copy)]
struct DwsimRow { temp: Option<f64> }

// Fungsi untuk mengirim query ke InfluxDB
fn post_influx(client: &Client, flux: String) -> Result<String> {
    let url = format!("{INFLUX_URL}/api/v2/query?org={ORG}");
    let resp = client
        .post(&url)
        .header("Authorization", format!("Token {}", TOKEN.trim()))
        .header("Accept", "application/csv")
        .header("Content-Type", "application/vnd.flux")
        .body(flux)
        .send()?;

    let status = resp.status();
    let body = resp.text().unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("Influx query FAILED: {} | {}", status, body.trim()));
    }
    println!("--- InfluxDB CSV Response ---\n{}\n-----------------------------", body.trim());
    Ok(body)
}

// Mengambil data terakhir menggunakan metode aggregateWindow (cara yang benar)
fn get_last_data(
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
  |> filter(fn: (r) => r["_field"] == "temperature" or r["_field"] == "humidity")
  |> aggregateWindow(every: {window}, fn: mean, createEmpty: false)
  |> group(columns: ["_field"])
  |> last()
"#);

    let csv = post_influx(client, flux)?;
    Ok(parse_influx_csv(&csv))
}

// Mengambil data temperature dari DWSIM_DATA bucket
fn get_dwsim_temperature(
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
    
    println!("🔍 Debug: Checking DWSIM bucket contents for measurement '{measurement}'...");
    if let Ok(debug_csv) = post_influx(client, debug_flux) {
        if debug_csv.trim().is_empty() || debug_csv.lines().count() <= 1 {
            println!("⚠️  DWSIM bucket '{bucket}' has no data for measurement '{measurement}' in last 24h");
            
            // Try to see what measurements exist
            let all_meas_flux = format!(r#"from(bucket: "{bucket}")
  |> range(start: -24h)  
  |> group(columns: ["_measurement"])
  |> distinct(column: "_measurement")
  |> limit(n: 10)
"#);
            if let Ok(meas_csv) = post_influx(client, all_meas_flux) {
                println!("Available measurements in bucket:");
                for line in meas_csv.lines() {
                    if !line.starts_with('#') && !line.contains("_measurement") {
                        println!("  - {}", line);
                    }
                }
            }
        } else {
            println!("✓ Found data in DWSIM bucket for measurement '{measurement}'");
        }
    }

    let flux = format!(r#"from(bucket: "{bucket}")
  |> range(start: {range})
  |> filter(fn: (r) => r["_measurement"] == "{measurement}")
  |> filter(fn: (r) => r["_field"] == "value")
  |> aggregateWindow(every: {window}, fn: mean, createEmpty: false)
  |> last()
"#);

    let csv = post_influx(client, flux)?;
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
                    if fname == "value" {
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
