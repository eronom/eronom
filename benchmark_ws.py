import asyncio
import base64
import time
import sys

def make_handshake(path, host):
    key = base64.b64encode(b"benchmarkkey1234").decode()
    return (
        f"GET {path} HTTP/1.1\r\n"
        f"Host: {host}\r\n"
        f"Upgrade: websocket\r\n"
        f"Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        f"Sec-WebSocket-Version: 13\r\n\r\n"
    ).encode()

async def ws_connection(host, port, path, num_messages, payload):
    reader, writer = await asyncio.open_connection(host, port)
    
    # Handshake
    writer.write(make_handshake(path, f"{host}:{port}"))
    await writer.drain()
    
    # Read response
    handshake_resp = b""
    while b"\r\n\r\n" not in handshake_resp:
        chunk = await reader.read(1024)
        if not chunk:
            break
        handshake_resp += chunk
        
    if b"101 Switching Protocols" not in handshake_resp:
        writer.close()
        await writer.wait_closed()
        raise Exception("Handshake failed")
        
    # Mask payload
    payload_bytes = payload.encode()
    mask_key = b"\x01\x02\x03\x04"
    masked_payload = bytes(b ^ mask_key[i % 4] for i, b in enumerate(payload_bytes))
    frame_header = bytes([0x81, 0x80 | len(payload_bytes)]) + mask_key
    frame = frame_header + masked_payload
    
    # Send and receive loop
    for _ in range(num_messages):
        writer.write(frame)
        await writer.drain()
        
        # Read reply (frame header + payload)
        header = await reader.readexactly(2)
        payload_len = header[1] & 0x7F
        if payload_len == 126:
            len_bytes = await reader.readexactly(2)
            payload_len = int.from_bytes(len_bytes, byteorder='big')
        elif payload_len == 127:
            len_bytes = await reader.readexactly(8)
            payload_len = int.from_bytes(len_bytes, byteorder='big')
            
        _ = await reader.readexactly(payload_len)
        
    writer.close()
    await writer.wait_closed()

async def run_benchmark(host, port, path, num_conns, num_messages, payload):
    start_time = time.time()
    tasks = []
    for _ in range(num_conns):
        tasks.append(ws_connection(host, port, path, num_messages, payload))
        
    results = await asyncio.gather(*tasks, return_exceptions=True)
    end_time = time.time()
    
    failures = 0
    for r in results:
        if isinstance(r, Exception):
            failures += 1
            
    elapsed = end_time - start_time
    total_messages = num_conns * num_messages
    success_conns = num_conns - failures
    success_messages = success_conns * num_messages
    
    print(f"Benchmark Results for ws://{host}:{port}{path}")
    print(f"  Connections: {num_conns} (successful: {success_conns}, failed: {failures})")
    print(f"  Messages per connection: {num_messages}")
    print(f"  Total messages sent/received: {success_messages}")
    print(f"  Time elapsed: {elapsed:.3f} seconds")
    print(f"  Throughput: {success_messages / elapsed:.1f} msg/sec")
    print()
    return success_messages / elapsed

if __name__ == '__main__':
    if len(sys.argv) < 3:
        print("Usage: python3 benchmark_ws.py <port> <conns> <msgs_per_conn>")
        sys.exit(1)
    port = int(sys.argv[1])
    conns = int(sys.argv[2])
    msgs = int(sys.argv[3])
    asyncio.run(run_benchmark('localhost', port, '/ws', conns, msgs, 'Hello Eronom'))
