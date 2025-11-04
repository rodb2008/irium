#!/usr/bin/env python3
import socket, threading

def handle_client(conn, addr):
    print(f"[+] Connection from {addr}")
    conn.sendall(b"Welcome to Irium test node\n")
    conn.close()

def main():
    host, port = "0.0.0.0", 8333
    print(f"[INFO] Listening on {host}:{port}")
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind((host, port))
        s.listen()
        while True:
            conn, addr = s.accept()
            threading.Thread(target=handle_client, args=(conn, addr)).start()

if __name__ == "__main__":
    main()
