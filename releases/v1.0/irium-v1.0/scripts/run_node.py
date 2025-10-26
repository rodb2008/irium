#!/usr/bin/env python3
import socket, threading, time, json, os
PORT = 38291
BOOT_TXT = "bootstrap/seedlist.txt"
BOOT_RT  = "bootstrap/seedlist.runtime"

def load_seeds():
    seeds = []
    for p in (BOOT_TXT, BOOT_RT):
        try:
            with open(p,"r") as f:
                for ln in f:
                    ln = ln.strip()
                    if ln and not ln.startswith("#"):
                        seeds.append(ln)
        except FileNotFoundError:
            pass
    return seeds

def handle(conn, addr):
    print(f"[+] connection from {addr}", flush=True)
    try:
        msg = {
            "banner": "Irium placeholder node",
            "time": int(time.time()),
            "seeds": load_seeds()[:8],
        }
        conn.sendall((json.dumps(msg)+"\n").encode())
    except Exception as e:
        print("[-] handler error:", e, flush=True)
    finally:
        try: conn.close()
        except: pass

def main():
    seeds = load_seeds()
    print(f"[i] loaded {len(seeds)} seeds", flush=True)
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("0.0.0.0", PORT))
    s.listen()
    print(f"[i] Irium placeholder listening on 0.0.0.0:{PORT}", flush=True)
    while True:
        c, a = s.accept()
        threading.Thread(target=handle, args=(c, a), daemon=True).start()

if __name__ == "__main__":
    main()
