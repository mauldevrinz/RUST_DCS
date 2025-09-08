use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::{self, PinDriver};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::uart::*;
use esp_idf_svc::hal::uart::config::{DataBits, StopBits, FlowControl};
use esp_idf_svc::wifi::{EspWifi, ClientConfiguration, Configuration as WifiConfiguration};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sntp::{EspSntp, SntpConf};
use esp_idf_svc::http::client::{EspHttpConnection, Configuration as HttpConfiguration};
use embedded_svc::http::client::Client;
use embedded_svc::http::Method;
use embedded_io::Write;
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::Result;

mod config;


fn calculate_crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for byte in data {
        crc ^= *byte as u16;
        for _ in 0..8 {
            if crc & 0x0001 != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}


fn test_network_connectivity() -> Result<()> {
    log::info!("🔍 Starting network connectivity test...");
    
    // Test basic connectivity first - try gateway
    match test_http_get("http://192.168.100.1") {
        Ok(_) => log::info!("✅ Gateway connectivity OK"),
        Err(e) => log::warn!("⚠️ Gateway test failed: {e:?}"),
    }
    
    // Test InfluxDB connectivity
    let influxdb_ping = format!("{}/ping", config::INFLUXDB_URL);
    match test_http_get(&influxdb_ping) {
        Ok(_) => {
            log::info!("✅ InfluxDB connectivity test passed!");
            Ok(())
        }
        Err(e) => {
            log::error!("❌ InfluxDB connectivity failed: {e:?}");
            Err(e)
        }
    }
}

fn test_http_get(url: &str) -> Result<()> {
    log::info!("Testing: {url}");
    
    let config = HttpConfiguration {
        timeout: Some(std::time::Duration::from_secs(10)),
        buffer_size: Some(1024),
        buffer_size_tx: Some(1024),
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        use_global_ca_store: true,
        ..Default::default()
    };
    
    let connection = EspHttpConnection::new(&config)
        .map_err(|e| anyhow::anyhow!("HTTP connection failed: {:?}", e))?;
    let mut client = Client::wrap(connection);
    
    let headers = [
        ("User-Agent", "ESP32-SHT20"),
        ("Connection", "close"),
        ("Accept", "*/*"),
    ];
    
    let request = client.request(Method::Get, url, &headers)
        .map_err(|e| anyhow::anyhow!("Request creation failed: {:?}", e))?;
    let response = request.submit()
        .map_err(|e| anyhow::anyhow!("Request submit failed: {:?}", e))?;
    let status = response.status();
    
    log::info!("Response: {url} - {status}");
    
    if status < 400 {
        log::info!("✅ {url}: OK");
        Ok(())
    } else {
        log::warn!("⚠️ {url}: HTTP {status}");
        anyhow::bail!("HTTP {}", status)
    }
}

fn send_to_influxdb(temperature: f32, humidity: f32) -> Result<()> {
    // Generate precise timestamp in nanoseconds for InfluxDB
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_nanos() as u64;
    
    // Simple InfluxDB line protocol: measurement,tag=value field1=value1,field2=value2 timestamp
    // Only temperature and humidity data, time-series based
    let line_protocol = format!(
        "sht20_sensor temperature={temperature:.2},humidity={humidity:.2} {timestamp}"
    );
    
    log::info!("Sending to InfluxDB: {line_protocol}");
    
    // More robust HTTP configuration with longer timeouts
    let config = HttpConfiguration {
        timeout: Some(std::time::Duration::from_secs(30)), // Increased timeout
        buffer_size: Some(2048),
        buffer_size_tx: Some(2048),
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        use_global_ca_store: true,
        ..Default::default()
    };
    
    // Retry logic for connection
    let max_retries = 3;
    for attempt in 1..=max_retries {
        log::info!("InfluxDB upload attempt {attempt}/{max_retries}");
        
        match try_influxdb_upload(&config, &line_protocol, attempt) {
            Ok(_) => {
                log::info!("✅ Data successfully sent to InfluxDB");
                return Ok(());
            }
            Err(e) => {
                log::error!("❌ InfluxDB upload attempt {attempt} failed: {e:?}");
                if attempt < max_retries {
                    log::info!("⏳ Retrying in {} seconds...", attempt * 2);
                    FreeRtos::delay_ms(attempt * 2 * 1000);
                } else {
                    log::error!("❌ All InfluxDB upload attempts failed");
                    return Err(e);
                }
            }
        }
    }
    
    anyhow::bail!("Maximum retry attempts exceeded")
}

fn try_influxdb_upload(config: &HttpConfiguration, line_protocol: &str, attempt: u32) -> Result<()> {
    let connection = EspHttpConnection::new(config)
        .map_err(|e| anyhow::anyhow!("HTTP connection failed: {:?}", e))?;
    let mut client = Client::wrap(connection);
    
    let url = format!(
        "{}/api/v2/write?org={}&bucket={}&precision=ns",
        config::INFLUXDB_URL,
        config::INFLUXDB_ORG,
        config::INFLUXDB_BUCKET
    );
    
    log::info!("Attempt {attempt}: Connecting to {url}");
    
    let auth_header = format!("Token {}", config::INFLUXDB_TOKEN);
    let headers = [
        ("Authorization", auth_header.as_str()),
        ("Content-Type", "text/plain"),
        ("Connection", "close"), // Force close connection after request
        ("User-Agent", "ESP32-SHT20/1.0"),
    ];
    
    let mut request = client.request(Method::Post, &url, &headers)
        .map_err(|e| anyhow::anyhow!("Request creation failed: {:?}", e))?;
    
    request.write_all(line_protocol.as_bytes())
        .map_err(|e| anyhow::anyhow!("Write request body failed: {:?}", e))?;
    request.flush()
        .map_err(|e| anyhow::anyhow!("Flush request failed: {:?}", e))?;
    
    let response = request.submit()
        .map_err(|e| anyhow::anyhow!("Submit request failed: {:?}", e))?;
    let status = response.status();
    
    log::info!("InfluxDB response status: {status}");
    
    match status {
        204 => Ok(()),
        400 => anyhow::bail!("Bad request - check data format"),
        401 => anyhow::bail!("Unauthorized - check token"),
        404 => anyhow::bail!("Not found - check URL/bucket/org"),
        _ => anyhow::bail!("HTTP {}", status)
    }
}

fn send_sensor_data(temperature: f32, humidity: f32) {
    // Log current time for debugging
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    log::info!("📊 Sensor Data at timestamp {now}:");
    log::info!("   Temperature: {temperature:.2}°C");  
    log::info!("   Humidity: {humidity:.2}%");
    
    // Try InfluxDB upload with improved error handling
    log::info!("🔄 Uploading to InfluxDB database...");
    match send_to_influxdb(temperature, humidity) {
        Ok(_) => {
            log::info!("✅ Data successfully stored in InfluxDB time-series database");
        }
        Err(e) => {
            // Detailed error analysis for troubleshooting
            let error_msg = format!("{e:?}");
            if error_msg.contains("timeout") || error_msg.contains("connection") {
                log::error!("🌐 Network connection failed - check WiFi and InfluxDB server");
            } else if error_msg.contains("401") || error_msg.contains("Unauthorized") {
                log::error!("🔐 Authentication failed - verify InfluxDB token in config.rs");
            } else if error_msg.contains("404") {
                log::error!("📍 Database not found - check InfluxDB URL/bucket/org settings");
            } else if error_msg.contains("400") {
                log::error!("📝 Data format error - check line protocol format");
            } else {
                log::error!("❌ InfluxDB upload failed: {e:?}");
            }
            
            log::info!("💾 Data preserved in ESP32 serial log for manual recovery");
        }
    }
}

fn setup_sntp() -> Result<()> {
    log::info!("🕐 Setting up SNTP for time synchronization...");
    
    let conf = SntpConf {
        servers: ["pool.ntp.org"],
        operating_mode: esp_idf_svc::sntp::OperatingMode::Poll,
        sync_mode: esp_idf_svc::sntp::SyncMode::Smooth,
    };
    
    let _sntp = EspSntp::new(&conf)?;
    
    // Wait for time sync with better validation
    log::info!("⏳ Waiting for time synchronization...");
    let mut attempts = 0;
    while attempts < 30 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Check if we have a reasonable timestamp (after Sept 2025)
        if now > 1725753600 { // Sept 8, 2025
            let datetime = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(now);
            log::info!("✅ Time synchronized! Current timestamp: {now}");
            log::info!("✅ Current time: {datetime:?}");
            return Ok(());
        }
        
        FreeRtos::delay_ms(1000);
        attempts += 1;
        if attempts % 5 == 0 {
            log::info!("⏳ Time sync attempt {attempts}/30 - current timestamp: {now}");
        }
    }
    
    log::warn!("⚠️ Time sync failed, using system time for timestamps");
    Ok(())
}

fn setup_wifi() -> Result<()> {
    log::info!("🔧 Initializing WiFi subsystem...");
    
    FreeRtos::delay_ms(2000);
    
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    
    log::info!("🔧 Creating WiFi driver...");
    let mut wifi = EspWifi::new(
        unsafe { esp_idf_svc::hal::modem::Modem::new() },
        sys_loop.clone(),
        Some(nvs)
    )?;
    
    let wifi_config = ClientConfiguration {
        ssid: config::WIFI_SSID.try_into().unwrap(),
        password: config::WIFI_PASSWORD.try_into().unwrap(),
        channel: None,
        bssid: None,
        auth_method: embedded_svc::wifi::AuthMethod::WPA2Personal,
        ..Default::default()
    };
    
    log::info!("🔧 Setting WiFi configuration...");
    wifi.set_configuration(&WifiConfiguration::Client(wifi_config))?;
    
    FreeRtos::delay_ms(1000);
    
    log::info!("🔧 Starting WiFi...");
    wifi.start()?;
    
    FreeRtos::delay_ms(3000);
    
    log::info!("📡 Connecting to WiFi '{}'...", config::WIFI_SSID);
    wifi.connect()?;
    
    let mut retry = 0;
    loop {
        let connected = wifi.is_connected().unwrap_or(false);
        log::info!("🔄 Connection check {}/60: {}", retry + 1, connected);
        
        if connected {
            log::info!("✅ WiFi connected successfully!");
            break;
        }
        if retry > 60 {
            anyhow::bail!("❌ Failed to connect to WiFi after 60 seconds");
        }
        
        FreeRtos::delay_ms(2000);
        retry += 1;
    }
    
    log::info!("⏳ Waiting for IP assignment...");
    FreeRtos::delay_ms(8000);
    
    if let Ok(ip_info) = wifi.sta_netif().get_ip_info() {
        log::info!("✅ ESP32 IP: {}", ip_info.ip);
        log::info!("✅ Gateway: {}", ip_info.subnet.gateway);
        log::info!("✅ Netmask: {}", ip_info.subnet.mask);
    } else {
        log::warn!("Could not get IP info");
    }
    
    log::info!("✅ WiFi setup completed");
    
    std::mem::forget(wifi);
    
    Ok(())
}

fn read_sht20_sensor(peripherals: Peripherals) {
    // Setup LED indicators for RX/TX activity
    let mut tx_led = PinDriver::output(peripherals.pins.gpio2).unwrap(); // Built-in LED for TX
    let mut rx_led = PinDriver::output(peripherals.pins.gpio4).unwrap(); // External LED for RX
    
    // Initially turn off LEDs
    tx_led.set_low().unwrap();
    rx_led.set_low().unwrap();
    
    log::info!("🔧 LED indicators configured - TX: GPIO2, RX: GPIO4");
    let config = UartConfig::new()
        .baudrate(9600.into())
        .data_bits(DataBits::DataBits8)
        .stop_bits(StopBits::STOP1)
        .flow_control(FlowControl::None);

    log::info!("🔧 Configuring UART1 - TX: GPIO16, RX: GPIO17, Baud: 9600");
    
    let uart = UartDriver::new(
        peripherals.uart1,
        peripherals.pins.gpio16, // TX
        peripherals.pins.gpio17, // RX  
        Option::<gpio::Gpio0>::None,
        Option::<gpio::Gpio0>::None,
        &config,
    ).unwrap();

    log::info!("✅ UART initialized for SHT20 communication");
    log::info!("🔧 RS485 Settings: Slave=0x01, Function=0x04, Baud=9600");
    log::info!("📡 InfluxDB Config - URL: {}, Org: {}, Bucket: {}", 
               config::INFLUXDB_URL, config::INFLUXDB_ORG, config::INFLUXDB_BUCKET);
    
    log::info!("⚡ Starting sensor reading loop...");

    let slave_addr = 0x01;
    let func_code = 0x04;

    loop {
        log::info!("🔄 Reading cycle started...");
        
        // Temperature reading
        let temp_cmd = [
            slave_addr, func_code, 0x00, 0x01, 0x00, 0x01
        ];
        let temp_crc = calculate_crc16(&temp_cmd);
        let temp_request = [
            temp_cmd[0], temp_cmd[1], temp_cmd[2], temp_cmd[3], temp_cmd[4], temp_cmd[5],
            (temp_crc & 0xFF) as u8, ((temp_crc >> 8) & 0xFF) as u8
        ];

        log::info!("📤 TX Temperature request: {temp_request:02X?}");
        
        // Turn on TX LED
        tx_led.set_high().unwrap();
        
        match uart.write(&temp_request) {
            Ok(bytes_written) => {
                log::info!("✅ TX: {bytes_written} bytes sent");
            }
            Err(e) => {
                log::error!("❌ TX Failed: {e:?}");
                tx_led.set_low().unwrap(); // Turn off TX LED on error
                FreeRtos::delay_ms(5000);
                continue;
            }
        }
        
        // Turn off TX LED after transmission
        tx_led.set_low().unwrap();
        
        FreeRtos::delay_ms(500);
        
        log::info!("⏳ Waiting for temperature response...");
        let mut temp_response = [0u8; 16];
        let mut temperature_raw = None;
        
        match uart.read(&mut temp_response, 3000) {
            Ok(bytes_read) => {
                if bytes_read > 0 {
                    // Turn on RX LED when data received
                    rx_led.set_high().unwrap();
                }
                log::info!("📥 RX: {bytes_read} bytes received");
                if bytes_read > 0 {
                    log::info!("📥 RX Temperature response: {:02X?}", &temp_response[..bytes_read]);
                    if bytes_read >= 7 {
                        let response_crc = ((temp_response[6] as u16) << 8) | (temp_response[5] as u16);
                        let calculated_crc = calculate_crc16(&temp_response[..5]);
                        log::info!("🔍 CRC check: received=0x{response_crc:04X}, calculated=0x{calculated_crc:04X}");
                        if response_crc == calculated_crc {
                            temperature_raw = Some(((temp_response[3] as u16) << 8) | (temp_response[4] as u16));
                            log::info!("✅ Temperature raw value: {}", temperature_raw.unwrap());
                        } else {
                            log::warn!("❌ CRC mismatch for temperature");
                        }
                    } else {
                        log::warn!("⚠️ Temperature response too short: {bytes_read} bytes");
                    }
                } else {
                    log::warn!("⚠️ No temperature response received");
                }
            }
            Err(e) => {
                log::error!("❌ RX Temperature error: {e:?}");
            }
        }
        
        // Turn off RX LED after processing
        rx_led.set_low().unwrap();

        FreeRtos::delay_ms(100);

        let hum_cmd = [
            slave_addr, func_code, 0x00, 0x00, 0x00, 0x01
        ];
        let hum_crc = calculate_crc16(&hum_cmd);
        let hum_request = [
            hum_cmd[0], hum_cmd[1], hum_cmd[2], hum_cmd[3], hum_cmd[4], hum_cmd[5],
            (hum_crc & 0xFF) as u8, ((hum_crc >> 8) & 0xFF) as u8
        ];

        if uart.write(&hum_request).is_err() {
            FreeRtos::delay_ms(5000);
            continue;
        }
        
        FreeRtos::delay_ms(200);
        
        let mut hum_response = [0u8; 16];
        let mut humidity_raw = None;
        
        if let Ok(bytes_read) = uart.read(&mut hum_response, 3000) {
            if bytes_read >= 7 {
                let response_crc = ((hum_response[6] as u16) << 8) | (hum_response[5] as u16);
                let calculated_crc = calculate_crc16(&hum_response[..5]);
                if response_crc == calculated_crc {
                    humidity_raw = Some(((hum_response[3] as u16) << 8) | (hum_response[4] as u16));
                }
            }
        }

        if humidity_raw.is_none() {
            FreeRtos::delay_ms(100);
            let hum_cmd2 = [
                slave_addr, func_code, 0x00, 0x02, 0x00, 0x01
            ];
            let hum_crc2 = calculate_crc16(&hum_cmd2);
            let hum_request2 = [
                hum_cmd2[0], hum_cmd2[1], hum_cmd2[2], hum_cmd2[3], hum_cmd2[4], hum_cmd2[5],
                (hum_crc2 & 0xFF) as u8, ((hum_crc2 >> 8) & 0xFF) as u8
            ];

            if uart.write(&hum_request2).is_ok() {
                FreeRtos::delay_ms(200);
                if let Ok(bytes_read) = uart.read(&mut hum_response, 3000) {
                    if bytes_read >= 7 {
                        let response_crc = ((hum_response[6] as u16) << 8) | (hum_response[5] as u16);
                        let calculated_crc = calculate_crc16(&hum_response[..5]);
                        if response_crc == calculated_crc {
                            humidity_raw = Some(((hum_response[3] as u16) << 8) | (hum_response[4] as u16));
                        }
                    }
                }
            }
        }

        match (temperature_raw, humidity_raw) {
            (Some(temp_raw), Some(hum_raw)) => {
                let temperature_offset = -1.2;
                let humidity_offset = -6.5;
                
                let temperature = (temp_raw as f32 / 10.0) + temperature_offset;
                let humidity = (hum_raw as f32 / 10.0) + humidity_offset;
                
                log::info!("SHT20 Sensor Reading:");
                log::info!("Temperature: {temperature:.2}°C");
                log::info!("Humidity: {humidity:.2}%");
                
                // Only send data if both values are valid
                if temperature > -50.0 && temperature < 100.0 && humidity > 0.0 && humidity < 100.0 {
                    send_sensor_data(temperature, humidity);
                } else {
                    log::warn!("⚠️ Sensor readings out of valid range, skipping upload");
                }
            }
            (Some(temp_raw), None) => {
                let temperature = (temp_raw as f32 / 10.0) - 1.2;
                log::info!("Temperature: {temperature:.2}°C, Humidity: N/A");
                log::warn!("⚠️ Incomplete sensor data - not uploading to InfluxDB");
            }
            (None, Some(hum_raw)) => {
                let humidity = (hum_raw as f32 / 10.0) - 6.5;
                log::info!("Temperature: N/A, Humidity: {humidity:.2}%");
                log::warn!("⚠️ Incomplete sensor data - not uploading to InfluxDB");
            }
            (None, None) => {
                log::warn!("Failed to read both temperature and humidity");
            }
        }

        // Wait 10 seconds between readings for better time-series data
        FreeRtos::delay_ms(10000);
    }
}

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("🚀 SHT20 Temperature & Humidity Data Logger");
    log::info!("📡 InfluxDB Target: {}", config::INFLUXDB_URL);
    log::info!("📊 Database: {} | Organization: {}", config::INFLUXDB_BUCKET, config::INFLUXDB_ORG);
    log::info!("🔄 Data will be collected every 10 seconds with precise timestamps");

    let peripherals = Peripherals::take().unwrap();
    
    
    match setup_wifi() {
        Ok(_) => {
            log::info!("✅ WiFi initialized successfully!");
            
            // Setup time synchronization
            match setup_sntp() {
                Ok(_) => {
                    log::info!("✅ Time synchronization completed!");
                }
                Err(e) => {
                    log::warn!("⚠️ Time sync failed: {e:?}");
                }
            }
            
            log::info!("🚀 Starting sensor reading with InfluxDB upload...");
            
            match test_network_connectivity() {
                Ok(_) => {
                    log::info!("✅ Network connectivity verified!");
                }
                Err(e) => {
                    log::warn!("⚠️ Network test failed: {e:?}");
                    log::info!("Continuing anyway...");
                }
            }
            
        }
        Err(e) => {
            log::error!("❌ WiFi setup failed: {e:?}");
            log::info!("Continuing in offline mode...");
        }
    }
    
    read_sht20_sensor(peripherals);
}