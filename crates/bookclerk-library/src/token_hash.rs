//! Hashing for claim tickets and session cookies (store hashes only).

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

use crate::master_key::MasterKey;

/// SHA-256 hex digest of a raw ticket/session token for durable storage.
///
/// # Arguments
///
/// * `raw` - Opaque plaintext token; callers must not persist this value.
///
/// # Returns
///
/// Lowercase hex SHA-256 digest (64 characters).
#[must_use]
pub fn hash_token(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hex::encode(hasher.finalize())
}

/// Domain-separated HMAC-SHA256 label for claim-ticket session derivation.
const CLAIM_SESSION_MAC_DOMAIN: &[u8] = b"bookclerk-claim-session-v1:";

/// Derive the portal session token for a claim ticket.
///
/// The raw session is `HMAC-SHA256(DEK, "bookclerk-claim-session-v1:" || ticket)`
/// hex-encoded (64 characters, matching a random 32-byte session token).
/// The same ticket plus process DEK always yields the same session, so a browser
/// retry after a lost `dbAtomic` reply reuses the `redeemClaimTicket:{token}:{session}`
/// operation id instead of minting a new one. The plaintext bearer is never written
/// to a receipt; only [`hash_token`] of this value is stored.
///
/// # Arguments
///
/// * `dek` - Process data-encryption key from [`crate::require_master_key`].
/// * `raw_ticket` - Plaintext claim ticket from the SPA / share URL.
#[must_use]
pub fn derive_claim_session_token(dek: &MasterKey, raw_ticket: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(dek.as_bytes())
        .unwrap_or_else(|_| unreachable!("HMAC-SHA256 accepts a 32-byte DEK"));
    mac.update(CLAIM_SESSION_MAC_DOMAIN);
    mac.update(raw_ticket.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_token_is_stable_hex() {
        let a = hash_token("example-token");
        let b = hash_token("example-token");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert_ne!(hash_token("other"), a);
    }

    #[test]
    fn derive_claim_session_token_is_stable_and_domain_separated() {
        let dek = crate::MasterKey::from_test_bytes([7u8; 32]);
        let a = derive_claim_session_token(&dek, "ticket-one");
        let b = derive_claim_session_token(&dek, "ticket-one");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert_ne!(derive_claim_session_token(&dek, "ticket-two"), a);
        let other_dek = crate::MasterKey::from_test_bytes([8u8; 32]);
        assert_ne!(derive_claim_session_token(&other_dek, "ticket-one"), a);
    }
}
