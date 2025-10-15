# ⛽ What Happens to Gas Fees in Irium?

## 💰 Gas Fee: 0.0001 IRM (10,000 satoshis)

## 🔄 Gas Fee Flow

### When You Send a Transaction:

1. **User Pays**:
   - Amount to send: X IRM
   - Gas fee: 0.0001 IRM
   - **Total cost**: X + 0.0001 IRM

2. **Transaction Created**:
   - Input: Your UTXO (with enough balance)
   - Output 1: Recipient gets X IRM
   - Output 2: Change back to you (if any)
   - **Fee**: Difference between inputs and outputs = 0.0001 IRM

3. **Transaction Goes to Mempool**:
   - Stored in `~/.irium/mempool/pending.json`
   - Waiting to be picked up by miners

4. **Miner Picks Up Transaction**:
   - Miner sees transaction in mempool
   - Includes it in next block template
   - **Miner gets the 0.0001 IRM fee as reward!**

5. **Block is Mined**:
   - Miner finds valid block
   - Block includes your transaction
   - **Miner receives**:
     - Block reward: 50 IRM (subsidy)
     - Transaction fees: 0.0001 IRM (your fee)
     - **Total**: 50.0001 IRM

6. **Transaction Confirmed**:
   - Block added to blockchain
   - Your transaction is confirmed
   - Recipient receives X IRM
   - **Miner keeps the 0.0001 IRM fee**

## 🎯 Who Gets the Gas Fees?

### **MINERS GET ALL GAS FEES! 💎**

**Why?**
- Incentivizes miners to include your transaction
- Compensates miners for processing
- Helps secure the network
- Standard in all PoW blockchains

## 📊 Fee Breakdown Example

### Example: Send 10 IRM

**Your wallet:**
- Balance before: 100 IRM
- Send amount: 10 IRM
- Gas fee: 0.0001 IRM
- **Balance after**: 89.9999 IRM

**Recipient:**
- Receives: 10 IRM

**Miner:**
- Block reward: 50 IRM
- Your gas fee: 0.0001 IRM
- Other tx fees: 0.0005 IRM (from other transactions)
- **Total miner reward**: 50.0006 IRM

## 💡 How It Compares

### Bitcoin:
- **Fee**: $0.50 - $5.00
- **Goes to**: Miner
- **Block reward**: 6.25 BTC (~$250,000)

### Litecoin:
- **Fee**: $0.01 - $0.10
- **Goes to**: Miner
- **Block reward**: 12.5 LTC (~$1,000)

### Irium:
- **Fee**: 0.0001 IRM (~$0.0001 if 1 IRM = $1)
- **Goes to**: Miner
- **Block reward**: 50 IRM

## 🔮 Future: Optional Relay Rewards

### Irium's Innovation (Not Yet Implemented):
Miners can optionally share a small % of fees with the peer who first relayed the transaction.

**Example:**
- Transaction fee: 0.0001 IRM
- Miner keeps: 0.00009 IRM (90%)
- Relay peer gets: 0.00001 IRM (10%)

**Benefits:**
- Incentivizes fast transaction relay
- Rewards nodes for good connectivity
- No inflation (comes from existing fees)
- Optional, not mandatory

**Status**: ⏳ Not yet implemented (Phase 8)

## 🎯 Gas Fee Economics

### Why 0.0001 IRM?

**Advantages:**
- ✅ Ultra-low for users (encourages adoption)
- ✅ Still profitable for miners (50 IRM block reward)
- ✅ Prevents spam (not free)
- ✅ Predictable (fixed fee)

**Comparison:**
- 1000x cheaper than Bitcoin
- 100x cheaper than Litecoin
- Encourages high transaction volume

### As Irium Grows:

**Early Stage (Now):**
- Block reward: 50 IRM (main incentive)
- Transaction fees: 0.0001 IRM each
- Few transactions per block
- Miners earn mostly from block reward

**Future (High Usage):**
- Block reward: Still 50 IRM (until halving)
- Transaction fees: 0.0001 IRM each
- Many transactions per block (1000+)
- Miners earn: 50 IRM + (1000 × 0.0001) = 50.1 IRM

**After Halvings:**
- Block reward decreases (25, 12.5, 6.25...)
- Transaction fees become more important
- High volume compensates for lower block reward

## 💎 Miner Economics

### Per Block:
- **Block reward**: 50 IRM
- **Transaction fees**: 0.0001 IRM × number of transactions
- **Block time**: 10 minutes
- **Blocks per day**: 144
- **Daily mining reward**: 7,200 IRM + fees

### Example Scenarios:

**Low Activity (10 tx/block):**
- Per block: 50 + (10 × 0.0001) = 50.001 IRM
- Per day: 7,200.144 IRM

**Medium Activity (100 tx/block):**
- Per block: 50 + (100 × 0.0001) = 50.01 IRM
- Per day: 7,201.44 IRM

**High Activity (1000 tx/block):**
- Per block: 50 + (1000 × 0.0001) = 50.1 IRM
- Per day: 7,214.4 IRM

## 🔑 Key Points

### Gas Fees:
- ✅ **Paid by**: Transaction sender
- ✅ **Received by**: Miner who includes transaction in block
- ✅ **Amount**: 0.0001 IRM (fixed)
- ✅ **Purpose**: Incentivize miners, prevent spam

### Why Miners Want Your Transaction:
- Gets 0.0001 IRM fee
- More transactions = more fees
- Incentive to include your transaction quickly

### Why Users Love It:
- Ultra-low cost (0.0001 IRM)
- Predictable (not dynamic)
- 1000x cheaper than Bitcoin
- Encourages usage

## 🌟 Summary

**Gas fees in Irium:**
1. **You pay**: 0.0001 IRM per transaction
2. **Miner receives**: 0.0001 IRM when they mine your transaction
3. **Purpose**: Incentivize miners, secure network, prevent spam
4. **Result**: Ultra-low cost for users, profitable for miners

**This is standard in all PoW blockchains (Bitcoin, Litecoin, etc.)**

The innovation is that Irium's fees are **1000x cheaper** while still incentivizing miners! 🚀

---

**Your 0.0001 IRM fee goes directly to the miner who includes your transaction in a block.**
