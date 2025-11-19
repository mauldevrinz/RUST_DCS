import requests
import csv
import os
from datetime import datetime
from dotenv import load_dotenv

# Muat variabel dari file .env
load_dotenv()

# --- Konfigurasi ---
THINGSBOARD_HOST = os.getenv("THINGSBOARD_HOST", "demo.thingsboard.io")
THINGSBOARD_PORT = os.getenv("THINGSBOARD_PORT", "80")
THINGSBOARD_USERNAME = os.getenv("THINGSBOARD_USERNAME")
THINGSBOARD_PASSWORD = os.getenv("THINGSBOARD_PASSWORD")
DEVICE_ID = os.getenv("DEVICE_ID")
TELEMETRY_KEYS = os.getenv("TELEMETRY_KEYS", "temperature,humidity")

# Rentang waktu (dalam milidetik sejak epoch)
END_TS = int(datetime.now().timestamp() * 1000)
START_TS = END_TS - (24 * 60 * 60 * 1000)  # Ambil data 24 jam terakhir

OUTPUT_CSV_FILE = "telemetry_data.csv"

def get_jwt_token():
    """Mendapatkan token otentikasi (JWT) dari ThingsBoard."""
    if not THINGSBOARD_USERNAME or not THINGSBOARD_PASSWORD:
        print("Error: THINGSBOARD_USERNAME dan THINGSBOARD_PASSWORD harus diatur di file .env")
        return None

    url = f"http://{THINGSBOARD_HOST}:{THINGSBOARD_PORT}/api/auth/login"
    payload = {
        "username": THINGSBOARD_USERNAME,
        "password": THINGSBOARD_PASSWORD
    }
    try:
        print(f"Mencoba otentikasi ke {url}...")
        response = requests.post(url, json=payload)
        response.raise_for_status()
        print("Otentikasi berhasil!")
        return response.json().get("token")
    except requests.exceptions.RequestException as e:
        print(f"Error saat otentikasi: {e}")
        return None

def get_telemetry_data(token):
    """Mengambil data telemetri dari ThingsBoard."""
    if not DEVICE_ID:
        print("Error: DEVICE_ID harus diatur di file .env")
        return None

    url = f"http://{THINGSBOARD_HOST}:{THINGSBOARD_PORT}/api/plugins/telemetry/DEVICE/{DEVICE_ID}/values/timeseries"
    headers = {
        "X-Authorization": f"Bearer {token}"
    }
    params = {
        "keys": TELEMETRY_KEYS,
        "startTs": START_TS,
        "endTs": END_TS,
        "limit": 10000,
        "agg": "NONE"
    }
    try:
        print(f"Mengambil data telemetri untuk perangkat {DEVICE_ID}...")
        print(f"Kunci: {TELEMETRY_KEYS}")
        print(f"Dari: {datetime.fromtimestamp(START_TS/1000)} Hingga: {datetime.fromtimestamp(END_TS/1000)}")
        response = requests.get(url, headers=headers, params=params)
        response.raise_for_status()
        data = response.json()
        print(f"Berhasil mengambil data. Jumlah key: {len(data)}")
        print(f"Data yang diterima: {data}")
        return data
    except requests.exceptions.RequestException as e:
        print(f"Error saat mengambil data telemetri: {e}")
        return None

def write_to_csv(data):
    """Menulis data telemetri ke file CSV dalam format lebar."""
    if not data:
        print("Tidak ada data untuk ditulis.")
        return

    print("Memproses data untuk format CSV lebar...")
    # Struktur data sementara untuk mengelompokkan nilai berdasarkan timestamp
    processed_data = {}
    
    # Ambil header dari TELEMETRY_KEYS
    telemetry_keys = TELEMETRY_KEYS.split(',')
    header = ["timestamp"] + telemetry_keys

    # Isi struktur data sementara
    for key, values in data.items():
        for record in values:
            ts = record['ts']
            if ts not in processed_data:
                processed_data[ts] = {}
            processed_data[ts][key] = record['value']

    print(f"Menemukan {len(processed_data)} timestamp unik.")

    try:
        with open(OUTPUT_CSV_FILE, mode='w', newline='', encoding='utf-8') as csv_file:
            writer = csv.writer(csv_file)
            
            # Tulis header
            writer.writerow(header)
            
            # Tulis data baris per baris
            # Urutkan berdasarkan timestamp untuk menjaga kronologi
            for ts in sorted(processed_data.keys()):
                # Mulai baris dengan timestamp yang diformat
                row = [datetime.fromtimestamp(ts / 1000).isoformat()]
                # Tambahkan nilai telemetri sesuai urutan header
                for key in telemetry_keys:
                    value = processed_data[ts].get(key, '') # Gunakan string kosong jika data tidak ada
                    row.append(value)
                writer.writerow(row)
            
            print(f"Berhasil menulis {len(processed_data)} baris data ke {OUTPUT_CSV_FILE}")

    except IOError as e:
        print(f"Error saat menulis file CSV: {e}")

if __name__ == "__main__":
    print("--- Program Perekam Data Telemetri ThingsBoard ---")
    
    if not all([THINGSBOARD_USERNAME, THINGSBOARD_PASSWORD, DEVICE_ID, TELEMETRY_KEYS]):
        print("\nError: Pastikan semua variabel ini diatur dalam file .env Anda:")
        print("- THINGSBOARD_USERNAME")
        print("- THINGSBOARD_PASSWORD")
        print("- DEVICE_ID")
        print("- TELEMETRY_KEYS")
        exit(1)

    token = get_jwt_token()
    if token:
        telemetry_data = get_telemetry_data(token)
        if telemetry_data:
            write_to_csv(telemetry_data)
    
    print("--- Program Selesai ---")