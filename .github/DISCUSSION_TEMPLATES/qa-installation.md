# Installation and Setup Questions

Post your installation and setup questions here!

## Common Questions:

### Q: What are the system requirements?
**A:** Minimum 2GB RAM, 10GB disk space, Python 3.10+

### Q: How do I download Irium?
**A:** 
```bash
wget https://github.com/iriumlabs/irium/releases/download/v1.0.1/irium-bootstrap-v1.0.1.tar.gz
tar -xzf irium-bootstrap-v1.0.1.tar.gz
cd irium-bootstrap-v1.0.1
./install.sh
```

### Q: How do I start the node?
**A:**
```bash
python3 scripts/irium-node.py
```

### Q: How do I check if my node is syncing?
**A:**
```bash
curl http://localhost:8082/api/stats
```

---

**Have a different question?** Reply below or create a new discussion!
