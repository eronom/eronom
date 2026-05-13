import requests
import sys
import time
import json
import subprocess
import os

# Eronom Test Suite
# This script tests the Eronom Rust server and language logic

BASE_URL = "http://localhost:8080"
BINARY_PATH = "./target/debug/eronom"

def test_endpoint(name, path, expected_status=200):
    print(f"Testing {name} ({path})...", end=" ", flush=True)
    try:
        url = f"{BASE_URL}{path}"
        response = requests.get(url, timeout=2)
        if response.status_code == expected_status:
            print("\033[92m[PASSED]\033[0m")
            return response
        else:
            print(f"\033[91m[FAILED]\033[0m (Status: {response.status_code})")
            return None
    except Exception as e:
        print(f"\033[91m[ERROR]\033[0m (Server might not be running)")
        return None

def test_er_file(file_path):
    print(f"Running {file_path} through eronom...", end=" ", flush=True)
    if not os.path.exists(BINARY_PATH):
        print(f"\033[91m[SKIPPED]\033[0m (Binary not found at {BINARY_PATH})")
        return None
    
    try:
        result = subprocess.run([BINARY_PATH, file_path], capture_output=True, text=True, timeout=5)
        if result.returncode == 0:
            print("\033[92m[PASSED]\033[0m")
            return result.stdout
        else:
            print(f"\033[91m[FAILED]\033[0m (Code: {result.returncode})")
            print(f"  Error: {result.stderr.strip()}")
            return None
    except Exception as e:
        print(f"\033[91m[ERROR]\033[0m ({e})")
        return None

def main():
    print("\033[1;34m=== Eronom Integration Test Suite ===\033[0m\n")
    
    # 1. Test .er file execution
    print("--- Language Logic Tests ---")
    output = test_er_file("example-er/hello.er")
    if output:
        print(f"  - Output length: {len(output)} chars")
    
    print("\n--- Server Endpoint Tests ---")
    # 2. Test Root Page
    resp = test_endpoint("Root Page", "/")
    if resp and "HMR" in resp.text:
        print(f"  - Found HMR title in response")
    
    # 3. Test API
    resp = test_endpoint("API Todo", "/api/todo")
    if resp:
        try:
            data = resp.json()
            print(f"  - API returned {len(data)} todos")
        except:
            print("  - API did not return valid JSON (is the port complete?)")

    print("\n\033[1;34m=== End of Test ===\033[0m")

if __name__ == "__main__":
    main()

if __name__ == "__main__":
    main()
