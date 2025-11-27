use std::io::{BufRead, BufReader};
use std::time::Duration;
use serialport::SerialPort;
use anyhow::{Result, anyhow};
use log::{info, error, warn};

#[derive(Debug, Clone)]
pub struct SensorData {
    pub timestamp: u64,
    pub temperature: f32,
    pub humidity: f32,
    pub exhaust_fan_status: Option<bool>,
    pub pump_status: Option<bool>,
}

pub struct SerialMonitor {
    port_name: String,
    baud_rate: u32,
}

#[derive(Debug, Default)]
struct RelayStatus {
    exhaust_fan: Option<bool>,
    pump: Option<bool>,
}

impl SerialMonitor {
    pub fn new(port_name: String, baud_rate: u32) -> Self {
        Self {
            port_name,
            baud_rate,
        }
    }

    pub async fn start_monitoring<F>(&self, mut on_data: F) -> Result<()>
    where
        F: FnMut(SensorData) -> Result<()> + Send + 'static,
    {
        let port_name = self.port_name.clone();
        let baud_rate = self.baud_rate;

        tokio::task::spawn_blocking(move || {
            info!("Starting serial monitor on {} @ {} baud", port_name, baud_rate);

            loop {
                match serialport::new(&port_name, baud_rate)
                    .timeout(Duration::from_millis(15000))
                    .open()
                {
                    Ok(port) => {
                        info!("Serial port {} opened successfully", port_name);

                        if let Err(e) = Self::read_loop(port, &mut on_data) {
                            error!("Serial read loop error: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to open serial port {}: {}", port_name, e);
                    }
                }

                warn!("Serial connection lost. Retrying in 5 seconds...");
                std::thread::sleep(Duration::from_secs(5));
            }
        }).await?
    }

    fn read_loop<F>(mut port: Box<dyn SerialPort>, on_data: &mut F) -> Result<()>
    where
        F: FnMut(SensorData) -> Result<()>,
    {
        let mut reader = BufReader::new(&mut *port);
        let mut line = String::new();
        let mut relay_status = RelayStatus::default();
        let mut pending_sensor_data: Option<SensorData> = None;

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Ok(_) => {
                    let trimmed = line.trim();

                    if !trimmed.is_empty() {
                        info!("ESP32: {}", trimmed);
                    }

                    if let Some((exhaust_fan, pump)) = Self::parse_relay_status(trimmed) {
                        relay_status.exhaust_fan = Some(exhaust_fan);
                        relay_status.pump = Some(pump);
                        info!("Relay status updated: Exhaust Fan={}, Pump={}",
                              if exhaust_fan { "ON" } else { "OFF" },
                              if pump { "ON" } else { "OFF" });

                        if let Some(mut sensor_data) = pending_sensor_data.take() {
                            sensor_data.exhaust_fan_status = relay_status.exhaust_fan;
                            sensor_data.pump_status = relay_status.pump;

                            if let Err(e) = on_data(sensor_data) {
                                error!("Failed to process sensor data: {}", e);
                            }
                        }
                    }

                    if let Some(mut sensor_data) = Self::parse_sensor_data(trimmed) {
                        sensor_data.exhaust_fan_status = relay_status.exhaust_fan;
                        sensor_data.pump_status = relay_status.pump;

                        pending_sensor_data = Some(sensor_data.clone());

                        if let Err(e) = on_data(sensor_data) {
                            error!("Failed to process sensor data: {}", e);
                        }
                    }
                }
                Err(e) => {
                    return Err(anyhow!("Serial read error: {}", e));
                }
            }
        }
    }

    fn parse_sensor_data(line: &str) -> Option<SensorData> {
        // Parse format: "SENSOR_DATA|timestamp|temperature|humidity"
        if let Some(stripped) = line.strip_prefix("SENSOR_DATA|") {
            let parts: Vec<&str> = stripped.split('|').collect();
            if parts.len() == 3 {
                if let (Ok(timestamp), Ok(temperature), Ok(humidity)) = (
                    parts[0].parse::<u64>(),
                    parts[1].parse::<f32>(),
                    parts[2].parse::<f32>(),
                ) {
                    return Some(SensorData {
                        timestamp,
                        temperature,
                        humidity,
                        exhaust_fan_status: None, // Will be filled by relay status
                        pump_status: None, // Will be filled by relay status
                    });
                }
            }
        }
        None
    }

    fn parse_relay_status(line: &str) -> Option<(bool, bool)> {
        // Parse format: "RELAY_STATUS|exhaust_fan:ON|pump:OFF"
        if let Some(stripped) = line.strip_prefix("RELAY_STATUS|") {
            let parts: Vec<&str> = stripped.split('|').collect();
            if parts.len() == 2 {
                let exhaust_fan_part = parts[0].strip_prefix("exhaust_fan:").unwrap_or("");
                let pump_part = parts[1].strip_prefix("pump:").unwrap_or("");

                let exhaust_fan_on = exhaust_fan_part == "ON";
                let pump_on = pump_part == "ON";

                return Some((exhaust_fan_on, pump_on));
            }
        }
        None
    }
}

