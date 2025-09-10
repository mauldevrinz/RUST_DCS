#!/usr/bin/env python3
"""
DWSIM to InfluxDB Connector
Mengambil nilai dari stream Water_i di DWSIM dan mengirimnya ke InfluxDB
"""

import time
import sys

INFLUXDB_URL = "http://192.168.121.64:8086"  # Computer IP on WiFi network
INFLUXDB_ORG  = "ITS"  # Org name from InfluxDB
INFLUXDB_BUCKET = "SENSOR_DATA"
INFLUXDB_TOKEN  = "pFlhPKsrTfaJ6-iIKz46wwHuKPOkp8GBK_chLeWCxpTgeFryMn9feiUukWZe5DAm4ocDJUAlPlyBaw8zg9PDYQ=="

try:
    import clr
    import os
    import sys
    
    # Set environment variables for better compatibility
    os.environ["DOTNET_SYSTEM_GLOBALIZATION_INVARIANT"] = "1"
    os.environ["MONO_ENV_OPTIONS"] = "--debug"
    
    # Add DWSIM library path to system path
    dwsim_lib_path = "/usr/local/lib/dwsim"
    if os.path.exists(dwsim_lib_path):
        sys.path.insert(0, dwsim_lib_path)
        clr.AddReference(os.path.join(dwsim_lib_path, "DWSIM.Automation.dll"))
    else:
        # Fallback to standard reference
        clr.AddReference("DWSIM.Automation")
    
    from DWSIM.Automation import Automation2
    DWSIM_AVAILABLE = True
    print("DWSIM libraries loaded successfully")

except ImportError as e:
    print(f"Warning: DWSIM Python libraries not available: {e}")
    print("Make sure pythonnet and DWSIM are properly installed")
    DWSIM_AVAILABLE = False
except Exception as e:
    print(f"Error loading DWSIM: {e}")
    print("Possible solutions:")
    print("1. Make sure DWSIM is running")
    print("2. Try: sudo apt install mono-complete")
    print("3. Check DWSIM installation path")
    DWSIM_AVAILABLE = False

try:
    from influxdb_client import InfluxDBClient, Point, WritePrecision
    INFLUXDB_AVAILABLE = True
except ImportError:
    print("InfluxDB client library not installed. Run: pip install influxdb-client")
    INFLUXDB_AVAILABLE = False


class DWSIMConnector:
    """Class untuk menghubungkan ke DWSIM dan mengambil data"""

    def __init__(self):
        self.simulation = None
        self.automation = None

    def connect_to_dwsim(self):
        if not DWSIM_AVAILABLE:
            print("DWSIM libraries not available")
            return False

        try:
            print("Connecting to DWSIM...")
            self.automation = Automation2()
            simulations = self.automation.GetOpenedSimulations()

            if len(simulations) > 0:
                self.simulation = simulations[0]
                print(f"Connected to simulation: {self.simulation.Name}")
                return True
            else:
                print("No opened simulations found")
                return False

        except Exception as e:
            print(f"Error connecting to DWSIM: {e}")
            return False

    def get_water_i_values(self):
        """Ambil nilai dari stream Water_i"""
        if not self.simulation:
            print("Not connected to simulation")
            return None

        try:
            flowsheet_data = self.simulation.GetFlowsheetData()
            water_stream = None

            for obj_id in flowsheet_data:
                obj = flowsheet_data[obj_id]
                if hasattr(obj, "Name") and obj.Name == "Water_i":
                    water_stream = obj
                    break

            if not water_stream:
                print("Water_i stream not found")
                return None

            phase_props = water_stream.Phases[0].Properties
            values = {
                "temperature_celsius": phase_props.temperature - 273.15,
                "pressure_bar": phase_props.pressure / 100000,
                "mass_flow_kg_s": phase_props.massflow,
                "density_kg_m3": phase_props.density,
                "enthalpy_kj_kg": phase_props.enthalpy,
            }
            return values

        except Exception as e:
            print(f"Error getting Water_i values: {e}")
            return None


class InfluxDBUploader:
    """Class untuk upload data ke InfluxDB"""

    def __init__(self, url, org, bucket, token):
        if not INFLUXDB_AVAILABLE:
            raise RuntimeError("InfluxDB client not available")

        self.client = InfluxDBClient(url=url, token=token, org=org)
        self.write_api = self.client.write_api(write_options=None)
        self.bucket = bucket
        self.org = org

    def upload_data(self, values, stream_name="Water_i", simulation_name="MySimulation"):
        if not values:
            print("No data to upload")
            return False

        try:
            points = []
            for key, val in values.items():
                point = (
                    Point("dwsim_measurement")
                    .tag("stream", stream_name)
                    .tag("simulation", simulation_name)
                    .field(key, float(val))
                    .time(time.time_ns(), WritePrecision.NS)
                )
                points.append(point)

            self.write_api.write(bucket=self.bucket, org=self.org, record=points)
            print("Data uploaded successfully to InfluxDB")
            return True
        except Exception as e:
            print(f"Error uploading to InfluxDB: {e}")
            return False


def main():
    print("DWSIM to InfluxDB Connector")
    print("=" * 40)

    dwsim = DWSIMConnector()
    if not dwsim.connect_to_dwsim():
        return

    values = dwsim.get_water_i_values()
    if not values:
        print("Failed to get Water_i values")
        return

    print("\nWater_i Stream Values:")
    for param, value in values.items():
        print(f"  {param}: {value:.4f}")

    uploader = InfluxDBUploader(INFLUXDB_URL, INFLUXDB_ORG, INFLUXDB_BUCKET, INFLUXDB_TOKEN)
    uploader.upload_data(values)


def continuous_monitoring(interval=10):
    print(f"Starting continuous monitoring (interval: {interval} s)")
    print("Press Ctrl+C to stop")

    dwsim = DWSIMConnector()
    if not dwsim.connect_to_dwsim():
        return

    uploader = InfluxDBUploader(INFLUXDB_URL, INFLUXDB_ORG, INFLUXDB_BUCKET, INFLUXDB_TOKEN)

    try:
        while True:
            values = dwsim.get_water_i_values()
            if values:
                print(f"Time: {time.strftime('%Y-%m-%d %H:%M:%S')} - Uploading data...")
                uploader.upload_data(values)
            else:
                print(f"Time: {time.strftime('%Y-%m-%d %H:%M:%S')} - No data available")

            time.sleep(interval)

    except KeyboardInterrupt:
        print("\nMonitoring stopped")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "continuous":
        interval = int(sys.argv[2]) if len(sys.argv) > 2 else 10
        continuous_monitoring(interval)
    else:
        main()
