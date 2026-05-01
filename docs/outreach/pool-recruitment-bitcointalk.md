# Pool Operator Recruitment — Bitcointalk Post

---

**[ANN] Irium (IRM) — SHA-256d — Seeking Community Pool Operators**

---

**About Irium**

Irium is a SHA-256d proof-of-work blockchain with a built-in settlement layer for trustless escrow agreements. The chain is live, has been running continuously, and uses the same double-SHA256 algorithm as Bitcoin.

- Algorithm: SHA-256d
- Block time: 600 seconds
- Block reward: 50 IRM (halves every 210,000 blocks, same schedule as Bitcoin)
- Max supply: 100,000,000 IRM
- Current height: 20,296
- No premine. No admin keys. MIT licence.

Full chain spec: https://github.com/iriumlabs/irium/blob/main/docs/LISTING-APPLICATION.md

---

**Hardware Compatibility**

Every SHA-256d ASIC works with Irium — Antminer, Whatsminer, and all other Bitcoin-grade hardware. No firmware change required. Point at the Stratum endpoint and mine.

---

**Current Infrastructure**

A public SOLO pool is live at pool.iriumlabs.org:3333 (ASIC) and pool.iriumlabs.org:3335 (CPU/GPU). The network needs more pool operators to decentralise the infrastructure.

---

**Calling Pool Operators**

We are looking for experienced pool operators to run community Irium pools.

The node (`iriumd`) exposes a standard `getblocktemplate` REST endpoint. Any Stratum server that supports Bitcoin's GBT protocol can be adapted to work with it. Full documentation is provided.

**Pool operator guide:** https://github.com/iriumlabs/irium/blob/main/docs/POOL-OPERATOR.md

**Requirements:**
- 2 cores / 4 GB RAM / 40 GB SSD minimum
- Publicly reachable Stratum endpoint
- Standard SHA-256d getblocktemplate-capable pool software (ckpool, custom bridge, etc.)

**To apply for official listing:**
1. Run your pool and confirm it is accepting miners and producing valid block submissions
2. Open a GitHub issue: https://github.com/iriumlabs/irium/issues with title "Pool listing request: [pool name]"
3. Include your Stratum endpoint, fee model, and contact

Verified pools are listed on iriumlabs.org and in the official repository.

---

**Links**

- Website: https://www.iriumlabs.org
- GitHub: https://github.com/iriumlabs/irium
- Telegram: https://t.me/iriumlabs
- Public pool: https://www.iriumlabs.org/pool
- API docs: https://github.com/iriumlabs/irium/blob/main/docs/API.md
- Pool operator guide: https://github.com/iriumlabs/irium/blob/main/docs/POOL-OPERATOR.md

---
*Note: Ibrahim posts this manually to the appropriate Bitcointalk board (Mining > Mining Software > Pools). Do not post directly.*
