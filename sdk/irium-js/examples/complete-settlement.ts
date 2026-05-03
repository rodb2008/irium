/**
 * complete-settlement.ts
 *
 * Shows a complete offer-take-proof-release flow using the Irium JS SDK.
 * Run against a live node: npx ts-node --esm examples/complete-settlement.ts
 *
 * Required environment variables:
 *   IRIUM_NODE_URL   — e.g. http://127.0.0.1:38300
 *   IRIUM_TOKEN      — RPC token from ~/.irium/node.token
 */

import { IriumClient, formatIrm, satoshisToIrm } from "../src/index.js";

const nodeUrl = process.env["IRIUM_NODE_URL"] ?? "http://127.0.0.1:38300";
const token = process.env["IRIUM_TOKEN"];

const client = new IriumClient({ nodeUrl, token });

async function main() {
  console.log("=== Irium JS SDK — Complete Settlement Example ===\n");

  // 1. Node status
  const status = await client.getStatus();
  console.log(`Node height : ${status.height}`);
  console.log(`Network era : ${status.network_era}`);
  console.log(`Peers       : ${status.peer_count}`);
  console.log(`Node ID     : ${status.node_id.slice(0, 16)}...`);
  console.log();

  // 2. Current block height
  const height = await client.getHeight();
  console.log(`Current height: ${height}`);
  console.log();

  // 3. Balance for a known address
  const address = "Q9KxBRfrnb6v9Vb8vuHjwkZaxj3ZRhJWpg";
  const balance = await client.getBalance(address);
  console.log(`Balance for ${address}`);
  console.log(`  Total  : ${formatIrm(balance.balance)} IRM`);
  console.log(`  Mined  : ${formatIrm(balance.mined_balance)} IRM`);
  console.log(`  UTXOs  : ${balance.utxo_count}`);
  console.log();

  // 4. Browse offers
  const offers = await client.getOffers({ status: "open" });
  console.log(`Open offers: ${offers.length}`);
  for (const offer of offers.slice(0, 3)) {
    console.log(
      `  [${offer.offer_id}] ${formatIrm(offer.amount_irm)} IRM via ${offer.payment_method} — ${offer.status}`,
    );
  }
  console.log();

  // 5. Fee estimate
  const fee = await client.getFeeEstimate();
  console.log(`Fee estimate: ${JSON.stringify(fee)}`);
  console.log();

  // 6. Build an OTC policy template (seller side)
  const policyId = `sdk-demo-${Date.now()}`;
  const sellerPubkey = "03e918af472e63de044c983df9f09bae57d4c78a70998d5d5fded408672886f868";
  const agreementSeed = {
    seller: { pubkey: sellerPubkey },
    buyer: { pubkey: sellerPubkey }, // demo: same party
    amount_irm: 100_000_000,
  };

  const hashResult = await client.computeAgreementHash(agreementSeed);
  console.log(`Agreement hash: ${hashResult.agreement_hash}`);
  console.log();

  const template = await client.buildOtcTemplate({
    policy_id: policyId,
    agreement_hash: hashResult.agreement_hash,
    attestors: [{ pubkey: sellerPubkey }],
    release_proof_type: "service_completion",
    refund_deadline_height: height + 1000,
    notes: "SDK demo trade",
  });
  console.log("OTC template built successfully");
  console.log(`  Policy ID         : ${(template as { policy: { policy_id: string } }).policy?.policy_id ?? policyId}`);
  console.log(`  Requirement count : ${(template as { requirement_count: number }).requirement_count}`);
  console.log();

  // 7. List proofs for the agreement hash
  const proofs = await client.listProofs({
    agreement_hash: hashResult.agreement_hash,
    include_all: true,
  });
  console.log(`Proofs for agreement: ${proofs.proofs?.length ?? 0}`);
  console.log();

  // 8. Real-time events via WebSocket
  console.log("Connecting WebSocket — listening for block.new events (5 seconds)...");
  let blockCount = 0;
  const ws = client.subscribeEvents(["block.new"], (event) => {
    blockCount++;
    const data = event as { type: string; data: { height: number; hash: string } };
    console.log(`  [block.new] height=${data.data?.height} hash=${data.data?.hash?.slice(0, 16)}...`);
  });

  await new Promise((resolve) => setTimeout(resolve, 5000));
  client.closeEvents();
  console.log(`Received ${blockCount} block events in 5 seconds`);
  console.log();

  console.log("=== Example complete ===");
}

main().catch((err) => {
  console.error("Error:", err);
  process.exit(1);
});
