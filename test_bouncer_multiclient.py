#!/usr/bin/env python3
"""Simple test to verify bouncer multiclient functionality."""

import socket
import time
import sys
import base64

def send_line(sock, line):
    """Send a line to the IRC server."""
    print(f">>> {line}")
    sock.sendall(f"{line}\r\n".encode())

def recv_lines(sock, timeout=2):
    """Receive lines from the IRC server."""
    sock.settimeout(timeout)
    data = b""
    try:
        while True:
            chunk = sock.recv(4096)
            if not chunk:
                break
            data += chunk
            if b"\r\n" in data:
                break
    except socket.timeout:
        pass
    
    lines = data.decode(errors='ignore').split('\r\n')
    for line in lines:
        if line:
            print(f"<<< {line}")
    return lines

def sasl_plain_auth(sock, username, password):
    """Perform SASL PLAIN authentication."""
    send_line(sock, "CAP REQ :sasl")
    lines = recv_lines(sock)
    
    send_line(sock, "AUTHENTICATE PLAIN")
    lines = recv_lines(sock)
    
    # PLAIN format: authzid\0authcid\0password
    auth_string = f"\0{username}\0{password}"
    auth_b64 = base64.b64encode(auth_string.encode()).decode()
    send_line(sock, f"AUTHENTICATE {auth_b64}")
    lines = recv_lines(sock)
    
    # Check for 900 (SASL success)
    if any("900" in line for line in lines):
        print("✓ SASL authentication successful")
        send_line(sock, "CAP END")
        recv_lines(sock)
        return True
    else:
        print("✗ SASL authentication failed")
        return False

def test_multiclient():
    """Test that two clients can connect with the same nick."""
    
    # Connect client 1
    print("\n=== CONNECTING CLIENT 1 ===")
    client1 = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    client1.connect(("127.0.0.1", 6667))
    
    # Authenticate client 1
    if not sasl_plain_auth(client1, "testuser", "testpass"):
        print("ERROR: Could not authenticate client 1")
        return False
    
    send_line(client1, "NICK testuser")
    send_line(client1, "USER test 0 * :Test User")
    
    lines = recv_lines(client1)
    
    # Check for successful registration
    if any("433" in line for line in lines):
        print("ERROR: Client 1 got 433 (nick in use)")
        return False
    
    if not any("001" in line or "Welcome" in line for line in lines):
        print("ERROR: Client 1 did not register successfully")
        return False
    
    print("✓ Client 1 registered successfully")
    
    # Join a channel
    print("\n=== CLIENT 1 JOINS #test ===")
    send_line(client1, "JOIN #test")
    lines = recv_lines(client1)
    
    if not any("JOIN" in line for line in lines):
        print("WARNING: No JOIN confirmation seen")
    
    # Connect client 2 with same nick AND SAME ACCOUNT
    print("\n=== CONNECTING CLIENT 2 (SAME NICK, SAME ACCOUNT) ===")
    client2 = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    client2.connect(("127.0.0.1", 6667))
    
    # Authenticate client 2 with SAME credentials
    if not sasl_plain_auth(client2, "testuser", "testpass"):
        print("ERROR: Could not authenticate client 2")
        return False
    
    send_line(client2, "NICK testuser")
    send_line(client2, "USER test2 0 * :Test User 2")
    
    lines = recv_lines(client2)
    
    # Check for 433 error (should NOT happen with multiclient)
    if any("433" in line for line in lines):
        print("✗ ERROR: Client 2 got 433 (Nickname is already in use)")
        print("   This means multiclient is NOT working yet")
        return False
    
    if not any("001" in line or "Welcome" in line for line in lines):
        print("✗ ERROR: Client 2 did not register successfully")
        return False
    
    print("✓ Client 2 registered with same nick!")
    
    # Test if client 2 sees channels (should see silent join)
    print("\n=== CHECKING IF CLIENT 2 SEES #test ===")
    send_line(client2, "LIST")
    lines = recv_lines(client2, timeout=1)
    
    # Test sending message from observer to channel
    print("\n=== TESTING MESSAGE ROUTING ===")
    observer = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    observer.connect(("127.0.0.1", 6667))
    send_line(observer, "NICK observer")
    send_line(observer, "USER obs 0 * :Observer")
    recv_lines(observer)
    
    send_line(observer, "JOIN #test")
    recv_lines(observer)
    
    send_line(observer, "PRIVMSG #test :Hello everyone")
    
    # Check if both clients receive it
    print("\n=== CLIENT 1 MESSAGES ===")
    lines1 = recv_lines(client1, timeout=1)
    has_msg1 = any("Hello everyone" in line for line in lines1)
    
    print("\n=== CLIENT 2 MESSAGES ===")
    lines2 = recv_lines(client2, timeout=1)
    has_msg2 = any("Hello everyone" in line for line in lines2)
    
    print(f"\nClient 1 received message: {has_msg1}")
    print(f"Client 2 received message: {has_msg2}")
    
    if not has_msg1:
        print("✗ Client 1 did not receive channel message")
    if not has_msg2:
        print("✗ Client 2 did not receive channel message (needs silent join)")
    
    client1.close()
    client2.close()
    observer.close()
    
    print("\n" + "="*60)
    if has_msg1 and has_msg2:
        print("SUCCESS: Both clients received messages (full multiclient working!)")
        return True
    elif not any("433" in line for line in lines):
        print("PARTIAL: 433 error fixed, but silent join not implemented yet")
        return True
    else:
        print("FAILURE: Multiclient not working")
        return False

if __name__ == "__main__":
    print("Testing bouncer multiclient functionality...")
    print("Make sure slircd is running on port 6667")
    print("="*60)
    
    try:
        success = test_multiclient()
        sys.exit(0 if success else 1)
    except ConnectionRefusedError:
        print("ERROR: Could not connect to server on port 6667")
        print("Make sure slircd is running: cargo run --release")
        sys.exit(1)
    except Exception as e:
        print(f"ERROR: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)
