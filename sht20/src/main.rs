use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::{self, PinDriver};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::uart::*;
use esp_idf_svc::hal::uart::config::{DataBits, StopBits, FlowControl};
// WiFi and HTTP dependencies removed for offline mode
// use esp_idf_svc::wifi::{EspWifi, ClientConfiguration, Configuration as WifiConfiguration};
// use esp_idf_svc::eventloop::EspSystemEventLoop;
// use esp_idf_svc::nvs::EspDefaultNvsPartition;
// use esp_idf_svc::sntp::{EspSntp, SntpConf};
// use esp_idf_svc::http::client::{EspHttpConnection, Configuration as HttpConfiguration};
// use embedded_svc::http::client::Client;
// use embedded_svc::http::Method;
// use embedded_io::Write;
use std::time::{SystemTime, UNIX_EPOCH};



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


// All HTTP/InfluxDB functions removed for offline mode
// Network connectivity, HTTP requests, and InfluxDB upload functions
// are not needed when operating in offline serial-only mode

// Relay control logic based on sensor readings
fn control_relays(temperature: f32, humidity: f32, motor_relay: &mut PinDriver<'_, gpio::Gpio2, gpio::Output>, pump_relay: &mut PinDriver<'_, gpio::Gpio4, gpio::Output>) {
    // Control logic thresholds
    const TEMP_MOTOR_ON: f32 = 30.0;    // Turn on motor if temp > 30°C
    const TEMP_MOTOR_OFF: f32 = 25.0;   // Turn off motor if temp < 25°C
    const HUMIDITY_PUMP_ON: f32 = 40.0; // Turn on pump if humidity < 40%
    const HUMIDITY_PUMP_OFF: f32 = 60.0; // Turn off pump if humidity > 60%

    // Motor control based on temperature
    if temperature > TEMP_MOTOR_ON {
        motor_relay.set_high().unwrap();
        log::info!("🔥 Motor ON: Temperature {:.1}°C > {:.1}°C", temperature, TEMP_MOTOR_ON);
    } else if temperature < TEMP_MOTOR_OFF {
        motor_relay.set_low().unwrap();
        log::info!("❄️ Motor OFF: Temperature {:.1}°C < {:.1}°C", temperature, TEMP_MOTOR_OFF);
    }

    // Pump control based on humidity
    if humidity < HUMIDITY_PUMP_ON {
        pump_relay.set_high().unwrap();
        log::info!("💧 Pump ON: Humidity {:.1}% < {:.1}%", humidity, HUMIDITY_PUMP_ON);
    } else if humidity > HUMIDITY_PUMP_OFF {
        pump_relay.set_low().unwrap();
        log::info!("💦 Pump OFF: Humidity {:.1}% > {:.1}%", humidity, HUMIDITY_PUMP_OFF);
    }

    // Get relay status for serial output
    let motor_status = motor_relay.is_set_high();
    let pump_status = pump_relay.is_set_high();

    println!("RELAY_STATUS|motor:{}|pump:{}",
             if motor_status { "ON" } else { "OFF" },
             if pump_status { "ON" } else { "OFF" });
}

fn send_sensor_data(temperature: f32, humidity: f32) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    // Output data ke serial untuk gateway
    println!("SENSOR_DATA|{timestamp}|{temperature:.2}|{humidity:.2}");
    println!("INFLUX_LINE|sht20_sensor temperature={temperature:.2},humidity={humidity:.2} {timestamp}");
}

// SNTP functions removed for offline mode  
// fn setup_sntp() -> Result<()> {
//     SNTP time sync not needed for offline operation
// }

// WiFi setup functions removed for offline mode
// fn setup_wifi() -> Result<()> {
//     All WiFi related code commented out for offline operation
// }

fn read_sht20_sensor(peripherals: Peripherals) {
    // Setup relay controls for motor and pump
    let mut motor_relay = PinDriver::output(peripherals.pins.gpio2).unwrap(); // Motor relay
    let mut pump_relay = PinDriver::output(peripherals.pins.gpio4).unwrap();  // Pump relay

    // Setup LED indicators for status
    let mut tx_led = PinDriver::output(peripherals.pins.gpio18).unwrap(); // TX status LED
    let mut rx_led = PinDriver::output(peripherals.pins.gpio19).unwrap(); // RX status LED

    // Initially turn off relays and LEDs
    motor_relay.set_low().unwrap();
    pump_relay.set_low().unwrap();
    tx_led.set_low().unwrap();
    rx_led.set_low().unwrap();

    log::info!("Relay Control: Motor=GPIO2, Pump=GPIO4");
    log::info!("LED Status: TX=GPIO18, RX=GPIO19");
    let config = UartConfig::new()
        .baudrate(9600.into())
        .data_bits(DataBits::DataBits8)
        .stop_bits(StopBits::STOP1)
        .flow_control(FlowControl::None);

    let uart = UartDriver::new(
        peripherals.uart1,
        peripherals.pins.gpio16,
        peripherals.pins.gpio17,
        Option::<gpio::Gpio0>::None,
        Option::<gpio::Gpio0>::None,
        &config,
    ).unwrap();

    log::info!("UART ready - RS485 9600 baud");

    let slave_addr = 0x01;
    let func_code = 0x04;

    loop {
        // Temperature reading
        let temp_cmd = [
            slave_addr, func_code, 0x00, 0x01, 0x00, 0x01
        ];
        let temp_crc = calculate_crc16(&temp_cmd);
        let temp_request = [
            temp_cmd[0], temp_cmd[1], temp_cmd[2], temp_cmd[3], temp_cmd[4], temp_cmd[5],
            (temp_crc & 0xFF) as u8, ((temp_crc >> 8) & 0xFF) as u8
        ];
        
        // Turn on TX LED
        tx_led.set_high().unwrap();
        
        match uart.write(&temp_request) {
            Ok(_) => {},
            Err(e) => {
                log::error!("TX Failed: {e:?}");
                tx_led.set_low().unwrap();
                FreeRtos::delay_ms(5000);
                continue;
            }
        }
        
        // Turn off TX LED after transmission
        tx_led.set_low().unwrap();
        
        FreeRtos::delay_ms(500);
        
        let mut temp_response = [0u8; 16];
        let mut temperature_raw = None;
        
        match uart.read(&mut temp_response, 3000) {
            Ok(bytes_read) => {
                if bytes_read > 0 {
                    rx_led.set_high().unwrap();
                }
                if bytes_read >= 7 {
                    let response_crc = ((temp_response[6] as u16) << 8) | (temp_response[5] as u16);
                    let calculated_crc = calculate_crc16(&temp_response[..5]);
                    if response_crc == calculated_crc {
                        temperature_raw = Some(((temp_response[3] as u16) << 8) | (temp_response[4] as u16));
                    } else {
                        log::warn!("CRC mismatch - temperature");
                    }
                }
            }
            Err(e) => {
                log::error!("RX Temperature error: {e:?}");
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
                
                log::info!("T: {temperature:.1}°C, H: {humidity:.1}%");
                
                if temperature > -50.0 && temperature < 100.0 && humidity > 0.0 && humidity < 100.0 {
                    send_sensor_data(temperature, humidity);
                    control_relays(temperature, humidity, &mut motor_relay, &mut pump_relay);
                } else {
                    log::warn!("Invalid readings - skipped");
                }
            }
            (Some(temp_raw), None) => {
                let temperature = (temp_raw as f32 / 10.0) - 1.2;
                log::warn!("T: {temperature:.1}°C, H: N/A - incomplete data");
            }
            (None, Some(hum_raw)) => {
                let humidity = (hum_raw as f32 / 10.0) - 6.5;
                log::warn!("T: N/A, H: {humidity:.1}% - incomplete data");
            }
            (None, None) => {
                log::warn!("Sensor read failed");
            }
        }

        // Wait 10 seconds between readings for better time-series data
        FreeRtos::delay_ms(10000);
    }
}

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("SHT20 Data Logger - Serial Gateway Mode");
    log::info!("Serial output every 10 seconds");

    let peripherals = Peripherals::take().unwrap();
    read_sht20_sensor(peripherals);
}