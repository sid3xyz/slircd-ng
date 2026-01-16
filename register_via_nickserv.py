#!/usr/bin/env python3
"""Register test user via NickServ."""

import socket
import time

def send_line(sock, line):
    print(f">>> {line}")
    sock.sendall(f"{line}\r\n".encode())

def recv_lines(sock, timeout=2):
    sock.settimeout(timeout)
    data = b""
    try:
        while True:
            chunk = sock.recv(4096)
            if not chunk:
                break
            data += chunk
    except socket.timeout:
        pass
    
    lines = data.decode(errors='ignore').split('\r\n')
    for line in lines:
        if line:
            print(f"<<< {line}")
    return lines

# Connect
print("Connecting...")
sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.connect(("127.0.0.1", 6667))

# Register without auth
send_line(sock, "NICK testuser")
send_line(sock, "USER test 0 * :Test User")
time.sleep(1)
recv_lines(sock)

# Register account
send_line(sock, "PRIVMSG NickServ :REGISTER testpass testuser@example.com")
time.sleep(1)
lines = recv_lines(sock)

print("\n" + "="*60)
if any("registered" in line.lower() or "registered" in line for line in lines):
    print("SUCCESS: Account registered!")
else:
    print("Check output above")

sock.close()
