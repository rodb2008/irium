
use crate::decoder::address::pkh_to_address;

// ─── Constants mirrored from irium-source/src/tx.rs ─────────────────────────
// HTLCv1: [tag(1) version(1) hashalg(1) secret_hash(32) recip_pkh(20) refund_pkh(20) timeout_u64(8)]
pub const HTLC_V1_TAG: u8 = 0xc0;
pub const HTLC_V1_VERSION: u8 = 1;
pub const HTLC_V1_HASHALG_SHA256: u8 = 1;
pub const HTLC_V1_LEN: usize = 83;

// BTC Swap HTLC: [tag(1) version(1) confirmations_required(1)
//   recipient_pkh(20) refund_pkh(20) btc_recipient_pkh(20)
//   btc_amount_sats_u64(8) timeout_height_u64(8) funding_binding(8)]
pub const HTLC_BTC_SWAP_TAG: u8 = 0xc3;
pub const HTLC_BTC_SWAP_VERSION: u8 = 1;
pub const HTLC_BTC_SWAP_LEN: usize = 87;

// LTC Swap HTLC: same layout as BTC swap, different tag
pub const HTLC_LTC_SWAP_TAG: u8 = 0xc7;
pub const HTLC_LTC_SWAP_VERSION: u8 = 1;
pub const HTLC_LTC_SWAP_LEN: usize = 87;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HtlcVariant {
    IriumV1,
    BtcSwapV1,
    LtcSwapV1,
}

impl HtlcVariant {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IriumV1   => "irium_v1",
            Self::BtcSwapV1 => "btc_swap_v1",
            Self::LtcSwapV1 => "ltc_swap_v1",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HtlcParams {
    pub variant: HtlcVariant,
    /// SHA256 preimage hash. All-zeros for swap HTLCs (use payment proof, not preimage).
    pub secret_hash: [u8; 32],
    pub recipient_pkh: [u8; 20],
    pub recipient_addr: String,
    pub refund_pkh: [u8; 20],
    pub refund_addr: String,
    /// Block height after which the refund branch unlocks.
    pub timeout_height: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgrAnchorType {
    Fund,
    Release,
    Refund,
    MilestoneRelease,
    DisputeResolve,
}

impl AgrAnchorType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fund             => "fund",
            Self::Release          => "release",
            Self::Refund           => "refund",
            Self::MilestoneRelease => "milestone_release",
            Self::DisputeResolve   => "dispute_resolve",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgrAnchor {
    pub anchor_type: AgrAnchorType,
    /// 64-character lowercase hex agreement hash.
    pub agreement_hash: String,
    /// Set only for MilestoneRelease anchors.
    pub milestone_id: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ScriptClass {
    P2Pkh {
        pkh: [u8; 20],
        address: String,
    },
    Htlc(HtlcParams),
    OpReturn {
        data: Vec<u8>,
        anchor: Option<AgrAnchor>,
    },
    /// Irium auxiliary coinbase output: zero value, large raw data
    /// (carries sybil proofs or chain-layer extensions).
    IriumData,
    Unknown,
}

// ─── Classification ──────────────────────────────────────────────────────────

/// Classify a script_pubkey byte slice given the output's satoshi value.
/// The value is used only to identify IriumData zero-value outputs.
pub fn classify_script(script: &[u8], value: i64) -> ScriptClass {
    // P2PKH: exactly 25 bytes | 76 a9 14 <20-byte PKH> 88 ac
    if script.len() == 25
        && script[0] == 0x76
        && script[1] == 0xa9
        && script[2] == 0x14
        && script[23] == 0x88
        && script[24] == 0xac
    {
        let mut pkh = [0u8; 20];
        pkh.copy_from_slice(&script[3..23]);
        return ScriptClass::P2Pkh { address: pkh_to_address(&pkh), pkh };
    }

    // HTLCv1 (IRM settlement): 83 bytes, tags c0/01/01
    if script.len() == HTLC_V1_LEN
        && script[0] == HTLC_V1_TAG
        && script[1] == HTLC_V1_VERSION
        && script[2] == HTLC_V1_HASHALG_SHA256
    {
        return ScriptClass::Htlc(extract_htlc_v1(script));
    }

    // BTC Swap HTLC: 87 bytes, tag c3/01
    if script.len() == HTLC_BTC_SWAP_LEN
        && script[0] == HTLC_BTC_SWAP_TAG
        && script[1] == HTLC_BTC_SWAP_VERSION
    {
        return ScriptClass::Htlc(extract_swap_htlc(script, HtlcVariant::BtcSwapV1));
    }

    // LTC Swap HTLC: 87 bytes, tag c7/01
    if script.len() == HTLC_LTC_SWAP_LEN
        && script[0] == HTLC_LTC_SWAP_TAG
        && script[1] == HTLC_LTC_SWAP_VERSION
    {
        return ScriptClass::Htlc(extract_swap_htlc(script, HtlcVariant::LtcSwapV1));
    }

    // OP_RETURN: starts with 0x6a
    if !script.is_empty() && script[0] == 0x6a {
        let data = parse_op_return_data(script);
        let anchor = parse_agr_anchor(&data);
        return ScriptClass::OpReturn { data, anchor };
    }

    // Irium auxiliary data: zero-value, large non-OP_RETURN script
    if value == 0 && script.len() > 30 {
        return ScriptClass::IriumData;
    }

    ScriptClass::Unknown
}

// ─── Extraction helpers ───────────────────────────────────────────────────────

// HTLCv1: [0]tag [1]ver [2]hashalg [3..35]secret_hash [35..55]recip_pkh [55..75]refund_pkh [75..83]timeout_u64
fn extract_htlc_v1(script: &[u8]) -> HtlcParams {
    let mut secret_hash   = [0u8; 32];
    let mut recipient_pkh = [0u8; 20];
    let mut refund_pkh    = [0u8; 20];
    secret_hash.copy_from_slice(&script[3..35]);
    recipient_pkh.copy_from_slice(&script[35..55]);
    refund_pkh.copy_from_slice(&script[55..75]);
    let timeout_height = u64::from_le_bytes(script[75..83].try_into().unwrap());
    HtlcParams {
        variant: HtlcVariant::IriumV1,
        secret_hash,
        recipient_addr: pkh_to_address(&recipient_pkh),
        recipient_pkh,
        refund_addr: pkh_to_address(&refund_pkh),
        refund_pkh,
        timeout_height,
    }
}

// Swap HTLC: [0]tag [1]ver [2]confirmations [3..23]recip_pkh [23..43]refund_pkh
//   [43..63]foreign_recip_pkh [63..71]foreign_amount_u64 [71..79]timeout_u64 [79..87]binding
fn extract_swap_htlc(script: &[u8], variant: HtlcVariant) -> HtlcParams {
    let mut recipient_pkh = [0u8; 20];
    let mut refund_pkh    = [0u8; 20];
    recipient_pkh.copy_from_slice(&script[3..23]);
    refund_pkh.copy_from_slice(&script[23..43]);
    let timeout_height = u64::from_le_bytes(script[71..79].try_into().unwrap());
    HtlcParams {
        variant,
        secret_hash: [0u8; 32],
        recipient_addr: pkh_to_address(&recipient_pkh),
        recipient_pkh,
        refund_addr: pkh_to_address(&refund_pkh),
        refund_pkh,
        timeout_height,
    }
}

fn parse_op_return_data(script: &[u8]) -> Vec<u8> {
    if script.len() < 2 { return vec![]; }
    let declared_len = script[1] as usize;
    let end = (2 + declared_len).min(script.len());
    script[2..end].to_vec()
}

/// Parse an agreement anchor from raw OP_RETURN data bytes.
/// Wire format: `agr1:<type_char>:<64-lowercase-hex-hash>[:<milestone_id>]`
/// Types: f=fund  l=release  r=refund  m=milestone_release  x=dispute_resolve
fn parse_agr_anchor(data: &[u8]) -> Option<AgrAnchor> {
    if data.len() < 71 { return None; }
    if &data[..5] != b"agr1:" { return None; }
    let type_char = data[5];
    if data[6] != b':' { return None; }
    let hash_slice = &data[7..71];
    if !hash_slice.iter().all(|b| b.is_ascii_hexdigit()) { return None; }
    let agreement_hash = String::from_utf8(hash_slice.to_vec()).ok()?;
    let milestone_id = if data.len() > 72 && data[71] == b':' {
        String::from_utf8(data[72..].to_vec()).ok()
    } else {
        None
    };
    let anchor_type = match type_char {
        b'f' => AgrAnchorType::Fund,
        b'l' => AgrAnchorType::Release,
        b'r' => AgrAnchorType::Refund,
        b'm' => AgrAnchorType::MilestoneRelease,
        b'x' => AgrAnchorType::DisputeResolve,
        _    => return None,
    };
    Some(AgrAnchor { anchor_type, agreement_hash, milestone_id })
}

// ─── Tests ───────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    // Real P2PKH from block 30220 coinbase: miner QArYYTV3ub22Anzgi2kCkrivFNCkzfUfkY
    const P2PKH_30220: &[u8] = &[
        0x76, 0xa9, 0x14,
        0x95, 0x0f, 0xa3, 0xd5, 0x4a, 0xea, 0x13, 0xc0, 0x50, 0x2e,
        0x42, 0xcd, 0xc4, 0xd0, 0x2c, 0x71, 0x4c, 0x81, 0x0d, 0x9b,
        0x88, 0xac,
    ];

    #[test]
    fn classifies_known_p2pkh() {
        match classify_script(P2PKH_30220, 5_000_000_000) {
            ScriptClass::P2Pkh { address, .. } => {
                assert_eq!(address, "QArYYTV3ub22Anzgi2kCkrivFNCkzfUfkY");
            }
            other => panic!("expected P2Pkh, got: {other:?}"),
        }
    }

    fn build_htlc_v1(secret_hash: [u8; 32], recip_pkh: [u8; 20], refund_pkh: [u8; 20], timeout: u64) -> Vec<u8> {
        let mut s = vec![0u8; HTLC_V1_LEN];
        s[0] = HTLC_V1_TAG;
        s[1] = HTLC_V1_VERSION;
        s[2] = HTLC_V1_HASHALG_SHA256;
        s[3..35].copy_from_slice(&secret_hash);
        s[35..55].copy_from_slice(&recip_pkh);
        s[55..75].copy_from_slice(&refund_pkh);
        s[75..83].copy_from_slice(&timeout.to_le_bytes());
        s
    }

    #[test]
    fn classifies_htlc_v1() {
        let secret_hash = [0xaa; 32];
        let recip_pkh   = [0xbb; 20];
        let refund_pkh  = [0xcc; 20];
        let script = build_htlc_v1(secret_hash, recip_pkh, refund_pkh, 1500);
        match classify_script(&script, 1_000_000) {
            ScriptClass::Htlc(p) => {
                assert_eq!(p.variant, HtlcVariant::IriumV1);
                assert_eq!(p.timeout_height, 1500);
                assert_eq!(p.secret_hash, [0xaa; 32]);
                assert_eq!(p.recipient_pkh, [0xbb; 20]);
            }
            other => panic!("expected Htlc, got: {other:?}"),
        }
    }

    #[test]
    fn htlc_wrong_length_is_unknown() {
        let mut script = build_htlc_v1([0; 32], [0; 20], [0; 20], 0);
        script.push(0xff); // one extra byte -> wrong length
        match classify_script(&script, 1000) {
            ScriptClass::Unknown => {}
            other => panic!("expected Unknown, got: {other:?}"),
        }
    }

    #[test]
    fn classifies_btc_swap_htlc() {
        let mut script = [0u8; HTLC_BTC_SWAP_LEN];
        script[0] = HTLC_BTC_SWAP_TAG;
        script[1] = HTLC_BTC_SWAP_VERSION;
        let timeout: u64 = 50_000;
        script[71..79].copy_from_slice(&timeout.to_le_bytes());
        match classify_script(&script, 500_000_000) {
            ScriptClass::Htlc(p) => {
                assert_eq!(p.variant, HtlcVariant::BtcSwapV1);
                assert_eq!(p.timeout_height, 50_000);
            }
            other => panic!("expected BtcSwap HTLC, got: {other:?}"),
        }
    }

    #[test]
    fn classifies_ltc_swap_htlc() {
        let mut script = [0u8; HTLC_LTC_SWAP_LEN];
        script[0] = HTLC_LTC_SWAP_TAG;
        script[1] = HTLC_LTC_SWAP_VERSION;
        match classify_script(&script, 200_000) {
            ScriptClass::Htlc(p) => assert_eq!(p.variant, HtlcVariant::LtcSwapV1),
            other => panic!("expected LtcSwap HTLC, got: {other:?}"),
        }
    }

    #[test]
    fn classifies_op_return_no_anchor() {
        let script = [0x6a_u8, 0x05, b'h', b'e', b'l', b'l', b'o'];
        match classify_script(&script, 0) {
            ScriptClass::OpReturn { anchor: None, .. } => {}
            other => panic!("expected OpReturn(no anchor), got: {other:?}"),
        }
    }

    fn make_anchor_script(type_char: u8, hash: &str, milestone: Option<&str>) -> Vec<u8> {
        let mut payload = format!("agr1:{}:{}", type_char as char, hash).into_bytes();
        if let Some(ms) = milestone {
            payload.push(b':');
            payload.extend_from_slice(ms.as_bytes());
        }
        let mut script = vec![0x6a, payload.len() as u8];
        script.extend(payload);
        script
    }

    #[test]
    fn classifies_fund_anchor() {
        let hash = "abcd1234".repeat(8);
        let script = make_anchor_script(b'f', &hash, None);
        match classify_script(&script, 0) {
            ScriptClass::OpReturn { anchor: Some(a), .. } => {
                assert_eq!(a.anchor_type, AgrAnchorType::Fund);
                assert_eq!(a.agreement_hash, hash);
                assert!(a.milestone_id.is_none());
            }
            other => panic!("expected Fund anchor, got: {other:?}"),
        }
    }

    #[test]
    fn classifies_release_anchor() {
        let hash = "deadbeef".repeat(8);
        let script = make_anchor_script(b'l', &hash, None);
        match classify_script(&script, 0) {
            ScriptClass::OpReturn { anchor: Some(a), .. } => {
                assert_eq!(a.anchor_type, AgrAnchorType::Release);
            }
            other => panic!("expected Release anchor, got: {other:?}"),
        }
    }

    #[test]
    fn classifies_refund_anchor() {
        let hash = "cafebabe".repeat(8);
        let script = make_anchor_script(b'r', &hash, None);
        match classify_script(&script, 0) {
            ScriptClass::OpReturn { anchor: Some(a), .. } => {
                assert_eq!(a.anchor_type, AgrAnchorType::Refund);
            }
            other => panic!("expected Refund anchor, got: {other:?}"),
        }
    }

    #[test]
    fn classifies_milestone_anchor_with_id() {
        let hash = "aabbccdd".repeat(8);
        let script = make_anchor_script(b'm', &hash, Some("milestone-3"));
        match classify_script(&script, 0) {
            ScriptClass::OpReturn { anchor: Some(a), .. } => {
                assert_eq!(a.anchor_type, AgrAnchorType::MilestoneRelease);
                assert_eq!(a.milestone_id.as_deref(), Some("milestone-3"));
            }
            other => panic!("expected MilestoneRelease anchor, got: {other:?}"),
        }
    }

    #[test]
    fn anchor_rejects_non_hex_hash() {
        // Hash contains non-hex chars (Z)
        let payload = b"agr1:f:ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ";
        let mut script = vec![0x6a, payload.len() as u8];
        script.extend_from_slice(payload);
        match classify_script(&script, 0) {
            ScriptClass::OpReturn { anchor: None, .. } => {}
            other => panic!("expected no anchor, got: {other:?}"),
        }
    }

    #[test]
    fn anchor_rejects_short_payload() {
        let payload = b"agr1:f:tooshort";
        let mut script = vec![0x6a, payload.len() as u8];
        script.extend_from_slice(payload);
        match classify_script(&script, 0) {
            ScriptClass::OpReturn { anchor: None, .. } => {}
            other => panic!("expected no anchor, got: {other:?}"),
        }
    }

    #[test]
    fn classifies_irium_data() {
        let script = vec![0u8; 50];
        match classify_script(&script, 0) {
            ScriptClass::IriumData => {}
            other => panic!("expected IriumData, got: {other:?}"),
        }
    }

    #[test]
    fn large_nonzero_value_script_is_unknown() {
        let script = vec![0u8; 50];
        match classify_script(&script, 1000) {
            ScriptClass::Unknown => {}
            other => panic!("expected Unknown for nonzero-value large script, got: {other:?}"),
        }
    }

    #[test]
    fn short_unknown_script_is_unknown() {
        let script = vec![0xde, 0xad, 0xbe, 0xef];
        match classify_script(&script, 0) {
            ScriptClass::Unknown => {}
            other => panic!("expected Unknown, got: {other:?}"),
        }
    }

    #[test]
    fn empty_script_is_unknown() {
        match classify_script(&[], 0) {
            ScriptClass::Unknown => {}
            other => panic!("expected Unknown for empty script, got: {other:?}"),
        }
    }
}
