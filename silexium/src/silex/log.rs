use anyhow::{Result, anyhow};
use blake3::Hasher;
use ed25519_dalek::{Signature, Signer, SigningKey};

pub type Hash = [u8; 32];

pub fn decode_hash(hex_str: &str) -> Result<Hash> {
    let bytes = hex::decode(hex_str).map_err(|_| anyhow!("invalid hex"))?;
    if bytes.len() != 32 {
        return Err(anyhow!("invalid hash length"));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

pub fn encode_hash(hash: &Hash) -> String {
    hex::encode(hash)
}

pub fn leaf_hash(data: &Hash) -> Hash {
    let mut hasher = Hasher::new();
    hasher.update(&[0x00]);
    hasher.update(data);
    hasher.finalize().into()
}

pub fn node_hash(left: &Hash, right: &Hash) -> Hash {
    let mut hasher = Hasher::new();
    hasher.update(&[0x01]);
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

pub fn mth(leaves: &[Hash]) -> Hash {
    if leaves.is_empty() {
        return blake3::hash(&[]).into();
    }
    if leaves.len() == 1 {
        return leaf_hash(&leaves[0]);
    }
    let k = largest_power_of_two_less_than(leaves.len());
    let left = mth(&leaves[..k]);
    let right = mth(&leaves[k..]);
    node_hash(&left, &right)
}

pub fn inclusion_proof(leaves: &[Hash], index: usize) -> Result<Vec<Hash>> {
    if index >= leaves.len() {
        return Err(anyhow!("leaf index out of range"));
    }
    Ok(inclusion_proof_inner(leaves, index))
}

fn inclusion_proof_inner(leaves: &[Hash], index: usize) -> Vec<Hash> {
    if leaves.len() <= 1 {
        return Vec::new();
    }
    let k = largest_power_of_two_less_than(leaves.len());
    if index < k {
        let mut proof = inclusion_proof_inner(&leaves[..k], index);
        proof.push(mth(&leaves[k..]));
        proof
    } else {
        let mut proof = inclusion_proof_inner(&leaves[k..], index - k);
        proof.push(mth(&leaves[..k]));
        proof
    }
}

pub fn consistency_proof(leaves: &[Hash], old_size: usize, new_size: usize) -> Result<Vec<Hash>> {
    if old_size == 0 || old_size > new_size || new_size > leaves.len() {
        return Err(anyhow!("invalid tree sizes"));
    }
    Ok(consistency_proof_inner(leaves, old_size, new_size))
}

fn consistency_proof_inner(leaves: &[Hash], old_size: usize, new_size: usize) -> Vec<Hash> {
    if old_size == new_size {
        return Vec::new();
    }
    let k = largest_power_of_two_less_than(new_size);
    if old_size <= k {
        let mut proof = consistency_proof_inner(&leaves[..k], old_size, k);
        proof.push(mth(&leaves[k..new_size]));
        proof
    } else {
        let mut proof = consistency_proof_inner(&leaves[k..new_size], old_size - k, new_size - k);
        proof.insert(0, mth(&leaves[..k]));
        proof
    }
}

pub fn sign_sth_payload(signing_key: &SigningKey, payload: &[u8]) -> String {
    let signature: Signature = signing_key.sign(payload);
    hex::encode(signature.to_bytes())
}

pub fn sth_payload(tree_size: u64, root_hash: &str, timestamp: &str) -> Vec<u8> {
    format!("SILEXIUM-STH\n{tree_size}\n{root_hash}\n{timestamp}\n").into_bytes()
}

fn largest_power_of_two_less_than(n: usize) -> usize {
    let mut k = 1usize;
    while (k << 1) < n {
        k <<= 1;
    }
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_leaf(index: u8) -> Hash {
        blake3::hash(&[index]).into()
    }

    fn verify_inclusion(leaf: &Hash, proof: &[Hash], index: usize, tree_size: usize) -> Hash {
        let mut idx = index;
        let mut computed = leaf_hash(leaf);
        let mut size = tree_size;
        for sibling in proof {
            if idx % 2 == 1 || idx == size - 1 {
                computed = node_hash(sibling, &computed);
            } else {
                computed = node_hash(&computed, sibling);
            }
            idx /= 2;
            size = (size + 1) / 2;
        }
        computed
    }

    #[test]
    fn inclusion_roundtrip() {
        let leaves: Vec<Hash> = (0u8..8).map(hash_leaf).collect();
        let root = mth(&leaves);
        for idx in 0..leaves.len() {
            let proof = inclusion_proof(&leaves, idx).unwrap();
            let computed = verify_inclusion(&leaves[idx], &proof, idx, leaves.len());
            assert_eq!(computed, root);
        }
    }

    // Consistency proofs are generated for UVD to verify. Keep coverage focused
    // on the inclusion proof until the verifier is implemented here.
}
