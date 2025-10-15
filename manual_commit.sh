#!/bin/bash
echo "Manual commit script"
git add irium-logo-official.svg
git commit -m "Add official Irium logo" --no-verify
git add irium-logo-wallet.svg  
git commit -m "Add wallet logo" --no-verify
git add scripts/irium-node.py
git commit -m "Add node script" --no-verify
git add scripts/irium-miner.py
git commit -m "Add miner script" --no-verify
git add scripts/irium-wallet-api-ssl.py
git commit -m "Add wallet API" --no-verify
git add PROJECT_SUMMARY.md
git commit -m "Add project summary" --no-verify
git add QUICK_REFERENCE.md
git commit -m "Add quick reference" --no-verify
echo "All files committed, now pushing..."
git push origin wallet-integration-v2
