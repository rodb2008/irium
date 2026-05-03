export interface NetworkStatus {
  height: number;
  genesis_hash: string;
  network_era: string;
  network_era_description: string;
  peer_count: number;
  anchor_loaded: boolean;
  node_id: string;
  best_header_tip: { height: number; hash: string };
  persisted_height: number;
}

export interface Balance {
  address: string;
  pkh: string;
  balance: number;
  mined_balance: number;
  utxo_count: number;
  mined_blocks: number;
  height: number;
}

export interface Offer {
  offer_id: string;
  seller_address: string;
  seller_pubkey: string;
  amount_irm: number;
  payment_method: string;
  status: "open" | "taken" | "filled" | "expired";
  created_at: number;
  timeout_height: number;
  description?: string;
  min_reputation_score?: number;
}

export interface OfferFeed {
  count: number;
  exported_at: number;
  offers: Offer[];
}

export interface OfferFilters {
  status?: string;
  minAmount?: number;
  maxAmount?: number;
  paymentMethod?: string;
}

export interface AgreementParty {
  address: string;
  pubkey?: string;
}

export interface AgreementPolicy {
  type: string;
  [key: string]: unknown;
}

export interface Agreement {
  buyer: AgreementParty;
  seller: AgreementParty;
  amount_irm: number;
  policy: AgreementPolicy;
  timeout_height?: number;
  [key: string]: unknown;
}

export type AgreementLifecycleState =
  | "unfunded"
  | "funded"
  | "proof_pending"
  | "proof_submitted"
  | "satisfied"
  | "disputed"
  | "timeout"
  | "refunded";

export interface AgreementLifecycle {
  state: AgreementLifecycleState;
  funded: boolean;
  release_eligible: boolean;
  refund_eligible: boolean;
  funding_txid?: string;
  funding_vout?: number;
  funding_height?: number;
  proof_submitted?: boolean;
  disputed?: boolean;
  tip_height: number;
}

export interface AgreementStatus {
  agreement_hash: string;
  lifecycle: AgreementLifecycle;
}

export interface Proof {
  proof_id: string;
  agreement_hash: string;
  prover_pubkey: string;
  payload: unknown;
  submitted_at: number;
  status: "active" | "satisfied" | "expired" | "timeout" | "unsatisfied";
}

export interface ProofSubmitResult {
  proof_id: string;
  accepted: boolean;
  message?: string;
}

export interface Reputation {
  seller_address: string;
  seller_pubkey: string;
  completion_rate: number;
  dispute_rate: number;
  avg_proof_response_time: number;
  default_count: number;
  risk_signal: string;
  total_agreements: number;
  ranking_score: number;
}

// WebSocket event types
export type IriumEventType =
  | "agreement.funded"
  | "agreement.proof_submitted"
  | "agreement.satisfied"
  | "agreement.timeout"
  | "agreement.disputed"
  | "agreement.proof_reorged"
  | "proof.gossip_received"
  | "offer.created"
  | "offer.taken"
  | "block.new"
  | "peer.connected"
  | "peer.disconnected";

export interface IriumEvent<T = unknown> {
  type: IriumEventType;
  ts: number;
  data: T;
}

export interface BlockNewEvent {
  height: number;
  hash: string;
}

export interface AgreementEvent {
  agreement_hash: string;
  [key: string]: unknown;
}

export interface EventFilter {
  agreement_hash?: string;
}

export class IriumError extends Error {
  constructor(
    public readonly code: number,
    message: string,
  ) {
    super(message);
    this.name = "IriumError";
  }
}

export type ProofRecord = Proof;

export interface ProofPayload {
  [key: string]: unknown;
}
