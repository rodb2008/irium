# Transaction Questions & Issues

Post your transaction-related questions here!

## Common Transaction Questions:

### Q: How do I send IRM?
**A:** Use the wallet command:
```bash
python3 scripts/irium-wallet-proper.py send <recipient_address> <amount_in_IRM>
```

### Q: What are transaction fees?
**A:** Transactions include a small fee (typically 0.0001 IRM) to incentivize miners.

### Q: How long do transactions take?
**A:** Transactions typically confirm within 10 minutes (1 block).

### Q: Can I cancel a transaction?
**A:** No, once broadcast, transactions cannot be cancelled. Always double-check before sending!

### Q: My transaction is pending, what do I do?
**A:** Wait for it to be included in a block. Check the mempool:
```bash
curl http://207.244.247.86:8082/api/mempool
```

---

**Need help with a specific transaction?** Share the transaction details below (but never share private keys!).
