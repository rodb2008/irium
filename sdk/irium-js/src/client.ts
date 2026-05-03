import { IriumWsClient } from "./ws-client.js";
import type {
  NetworkStatus,
  Balance,
  Offer,
  OfferFilters,
  AgreementStatus,
  ProofRecord,
  IriumEventType,
  IriumEvent,
} from "./types.js";
import { IriumError } from "./types.js";

export interface IriumClientOptions {
  nodeUrl: string;
  token?: string;
}

type EventHandler<T extends IriumEvent = IriumEvent> = (event: T) => void;

export class IriumClient {
  private readonly baseUrl: string;
  private readonly headers: Record<string, string>;
  private wsClient?: IriumWsClient;

  constructor(options: IriumClientOptions) {
    this.baseUrl = options.nodeUrl.replace(/\/$/, "");
    this.headers = { "Content-Type": "application/json" };
    if (options.token) {
      this.headers["Authorization"] = `Bearer ${options.token}`;
    }
  }

  private async get<T>(path: string): Promise<T> {
    const res = await fetch(`${this.baseUrl}${path}`, { headers: this.headers });
    if (!res.ok) {
      throw new IriumError(res.status, await res.text());
    }
    return res.json() as Promise<T>;
  }

  private async post<T>(path: string, body: unknown): Promise<T> {
    const res = await fetch(`${this.baseUrl}${path}`, {
      method: "POST",
      headers: this.headers,
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      throw new IriumError(res.status, await res.text());
    }
    return res.json() as Promise<T>;
  }

  async getStatus(): Promise<NetworkStatus> {
    return this.get<NetworkStatus>("/status");
  }

  async getHeight(): Promise<number> {
    const status = await this.getStatus();
    return status.height;
  }

  async getBalance(address: string): Promise<Balance> {
    return this.get<Balance>(`/rpc/balance?address=${encodeURIComponent(address)}`);
  }

  async getOffers(filters?: OfferFilters): Promise<Offer[]> {
    const feed = await this.get<{ count: number; exported_at: number; offers: Offer[] }>("/offers/feed");
    let offers = feed.offers;
    if (filters?.status) {
      offers = offers.filter((o) => o.status === filters.status);
    }
    if (filters?.paymentMethod) {
      offers = offers.filter((o) => o.payment_method === filters.paymentMethod);
    }
    if (filters?.minAmount !== undefined) {
      offers = offers.filter((o) => o.amount_irm >= filters.minAmount!);
    }
    if (filters?.maxAmount !== undefined) {
      offers = offers.filter((o) => o.amount_irm <= filters.maxAmount!);
    }
    return offers;
  }

  async computeAgreementHash(agreement: object): Promise<{ agreement_hash: string }> {
    return this.post("/rpc/computeagreementhash", { agreement });
  }

  async getAgreementStatus(agreement: object): Promise<AgreementStatus> {
    return this.post("/rpc/agreementstatus", { agreement });
  }

  async getReleaseEligibility(params: {
    agreement: object;
    funding_txid: string;
    htlc_vout?: number;
    milestone_id?: string;
    destination_address?: string;
    fee_per_byte?: number;
    broadcast?: boolean;
  }): Promise<{ eligible: boolean; reasons: string[]; branch: string; funded: boolean }> {
    return this.post("/rpc/agreementreleaseeligibility", params);
  }

  async getRefundEligibility(params: {
    agreement: object;
    funding_txid: string;
    htlc_vout?: number;
    destination_address?: string;
    fee_per_byte?: number;
    broadcast?: boolean;
  }): Promise<{ eligible: boolean; reasons: string[]; branch: string; funded: boolean }> {
    return this.post("/rpc/agreementrefundeligibility", params);
  }

  async buildOtcTemplate(params: {
    policy_id: string;
    agreement_hash: string;
    attestors: Array<{ attestor_id: string; pubkey_hex: string; display_name?: string }>;
    release_proof_type: string;
    refund_deadline_height: number;
    threshold?: number;
    notes?: string;
  }): Promise<object> {
    return this.post("/rpc/buildotctemplate", params);
  }

  async createOffer(params: {
    policy_id: string;
    agreement_hash: string;
    attestors: Array<{ attestor_id: string; pubkey_hex: string; display_name?: string }>;
    release_proof_type: string;
    refund_deadline_height: number;
    threshold?: number;
    notes?: string;
  }): Promise<object> {
    return this.buildOtcTemplate(params);
  }

  async takeOffer(params: {
    offer_id: string;
    buyer_pubkey: string;
    buyer_address: string;
    [key: string]: unknown;
  }): Promise<object> {
    return this.post("/rpc/buildcontractortemplate", params);
  }

  async inspectAgreement(agreement: object): Promise<object> {
    return this.post("/rpc/inspectagreement", { agreement });
  }

  async fundAgreement(params: object): Promise<object> {
    return this.post("/rpc/fundagreement", params);
  }

  async submitProof(proof: object): Promise<{
    proof_id: string;
    agreement_hash: string;
    accepted: boolean;
    duplicate: boolean;
    message: string;
    tip_height: number;
    status: string;
  }> {
    return this.post("/rpc/submitproof", { proof });
  }

  async listProofs(params: {
    agreement_hash?: string;
    include_all?: boolean;
    active_only?: boolean;
  }): Promise<{ proofs: ProofRecord[] }> {
    return this.post("/rpc/listproofs", params);
  }

  async getProof(proofId: string): Promise<{ proof: ProofRecord | null }> {
    return this.post("/rpc/getproof", { proof_id: proofId });
  }

  async checkPolicy(params: object): Promise<object> {
    return this.post("/rpc/checkpolicy", params);
  }

  async storePolicy(policy: object, replace?: boolean): Promise<object> {
    return this.post("/rpc/storepolicy", { policy, replace: replace ?? false });
  }

  async getPolicy(agreementHash: string): Promise<object> {
    return this.post("/rpc/getpolicy", { agreement_hash: agreementHash });
  }

  async evaluatePolicy(params: object): Promise<object> {
    return this.post("/rpc/evaluatepolicy", params);
  }

  async getAgreementTimeline(agreement: object): Promise<object> {
    return this.post("/rpc/agreementtimeline", { agreement });
  }

  async getAgreementAudit(agreement: object): Promise<object> {
    return this.post("/rpc/agreementaudit", { agreement });
  }

  async submitTx(txHex: string): Promise<{ txid: string }> {
    return this.post("/rpc/submit_tx", { tx_hex: txHex });
  }

  async getBlock(height: number): Promise<object> {
    return this.get(`/rpc/block?height=${height}`);
  }

  async getBlockByHash(hash: string): Promise<object> {
    return this.get(`/rpc/block_by_hash?hash=${encodeURIComponent(hash)}`);
  }

  async getTx(txid: string): Promise<object> {
    return this.get(`/rpc/tx?txid=${encodeURIComponent(txid)}`);
  }

  async getNetworkHashrate(): Promise<object> {
    return this.get("/rpc/network_hashrate");
  }

  async getFeeEstimate(): Promise<{ fee_rate: number }> {
    return this.get("/rpc/fee_estimate");
  }

  subscribeEvents(
    eventTypes: IriumEventType[],
    handler: EventHandler,
    filter?: { agreement_hash?: string },
  ): IriumWsClient {
    const wsUrl = this.baseUrl.replace(/^http/, "ws") + "/ws";
    if (!this.wsClient) {
      this.wsClient = new IriumWsClient(wsUrl, this.headers["Authorization"]);
      this.wsClient.connect();
    }
    this.wsClient.subscribe(eventTypes, handler, filter);
    return this.wsClient;
  }

  closeEvents(): void {
    this.wsClient?.close();
    this.wsClient = undefined;
  }
}
