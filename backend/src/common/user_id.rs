use sha2::{Digest, Sha256};

pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

pub fn user_id_to_qdrant_id(user_id: &str) -> u64 {
    let digest = Sha256::digest(user_id.as_bytes());
    let be = u64::from_be_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ]);
    be % MAX_SAFE_INTEGER
}
