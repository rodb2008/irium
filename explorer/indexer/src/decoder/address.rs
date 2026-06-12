
use sha2::{Digest, Sha256};

/// Irium P2PKH version byte — produces Q- or P-prefix base58check addresses.
const IRIUM_P2PKH_VERSION: u8 = 0x39;

/// Encode a 20-byte public key hash into an Irium base58check address.
pub fn pkh_to_address(pkh: &[u8; 20]) -> String {
    let mut payload = [0u8; 25];
    payload[0] = IRIUM_P2PKH_VERSION;
    payload[1..21].copy_from_slice(pkh);
    let checksum = sha256d(&payload[..21]);
    payload[21..25].copy_from_slice(&checksum[..4]);
    bs58::encode(payload).into_string()
}

fn sha256d(data: &[u8]) -> [u8; 32] {
    let first = Sha256::digest(data);
    Sha256::digest(first).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_known_miner_address() {
        // Verified: /rpc/balance for QArYYTV3ub22Anzgi2kCkrivFNCkzfUfkY
        // returns pkh = 950fa3d54aea13c0502e42cdc4d02c714c810d9b
        let pkh: [u8; 20] = hex::decode("950fa3d54aea13c0502e42cdc4d02c714c810d9b")
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(pkh_to_address(&pkh), "QArYYTV3ub22Anzgi2kCkrivFNCkzfUfkY");
    }

    #[test]
    fn encodes_second_known_address() {
        // Verified: block 30222 coinbase, miner_address = QAWCsQpS7NqQqijQtehLhi6ddccLiJ1mM3
        // Coinbase script: 76a914 9136e5fbb51f60f9b14989b844dee17cd410ca9e 88ac
        let pkh: [u8; 20] = hex::decode("9136e5fbb51f60f9b14989b844dee17cd410ca9e")
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(pkh_to_address(&pkh), "QAWCsQpS7NqQqijQtehLhi6ddccLiJ1mM3");
    }

    #[test]
    fn address_has_correct_length() {
        let pkh = [0u8; 20];
        let addr = pkh_to_address(&pkh);
        // All 25-byte base58check with version 0x39 encode to 34 characters
        assert_eq!(addr.len(), 34, "address length should be 34, got: {addr}");
    }

    #[test]
    fn address_starts_with_q_or_p() {
        // Spot-check several PKH values for the expected prefix
        let test_cases: &[(&str, char)] = &[
            ("950fa3d54aea13c0502e42cdc4d02c714c810d9b", 'Q'),
            ("9136e5fbb51f60f9b14989b844dee17cd410ca9e", 'Q'),
        ];
        for (pkh_hex, expected_prefix) in test_cases {
            let pkh: [u8; 20] = hex::decode(pkh_hex).unwrap().try_into().unwrap();
            let addr = pkh_to_address(&pkh);
            assert_eq!(
                addr.chars().next().unwrap(), *expected_prefix,
                "address for pkh {pkh_hex} should start with {expected_prefix}, got: {addr}"
            );
        }
    }
}
