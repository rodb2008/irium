use k256::ecdsa::signature::Verifier;
use k256::ecdsa::{Signature, VerifyingKey};

/// Verify a DER-encoded secp256k1 signature against a 32-byte digest,
/// using a SEC1-encoded public key (compressed or uncompressed).
/// If the signature has a trailing sighash byte (0x01), it is stripped.
pub fn verify_der_signature(pubkey: &[u8], digest: &[u8; 32], signature: &[u8]) -> bool {
    if signature.len() < 8 {
        return false;
    }
    let sig_bytes = if let Some(last) = signature.last() {
        if *last == 0x01 {
            &signature[..signature.len() - 1]
        } else {
            signature
        }
    } else {
        return false;
    };

    let sig = match Signature::from_der(sig_bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };

    let vk = match VerifyingKey::from_sec1_bytes(pubkey) {
        Ok(v) => v,
        Err(_) => return false,
    };

    vk.verify(digest, &sig).is_ok()
}
