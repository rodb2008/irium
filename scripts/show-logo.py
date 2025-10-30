#!/usr/bin/env python3
"""Display Irium logo and network information."""

import os
import sys

def show_logo():
    logo_file = os.path.join(os.path.dirname(__file__), '..', 'irium-logo-clean.txt')
    if os.path.exists(logo_file):
        with open(logo_file, 'r') as f:
            print(f.read())
    else:
        print("IRIUM BLOCKCHAIN - Proof-of-Work SHA-256d")
        print("Max Supply: 100M IRM | Bootstrap: 207.244.247.86:19444")

def show_network_info():
    print("\n" + "="*60)
    print("🌐 NETWORK INFORMATION")
    print("="*60)
    print("🔗 Bootstrap Node: configured via BOOTSTRAP_NODES env
    print("🔐 Wallet API: configured by your deployment
    print("📱 Web3 Compatible: MetaMask, Trust Wallet")
    print("⚡ Consensus: Proof-of-Work (SHA-256d)")
    print("💰 Max Supply: 100,000,000 IRM")
    print("🔒 Genesis Vesting: 3,500,000 IRM (CLTV)")
    print("⏱️  Block Time: 600 seconds")
    print("🎯 Difficulty Retarget: Every 2016 blocks")
    print("="*60)

if __name__ == "__main__":
    show_logo()
    if len(sys.argv) > 1 and sys.argv[1] == "--network":
        show_network_info()
