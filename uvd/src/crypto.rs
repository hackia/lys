use crate::utils::{ko_audit_commit, ok, ok_audit_commit};
use ed25519_dalek::SECRET_KEY_LENGTH;
use ed25519_dalek::Signature;
use ed25519_dalek::SigningKey;
use ed25519_dalek::VerifyingKey;
use ed25519_dalek::{Signer, Verifier};
use rand::RngExt;
use sqlite::{Connection, State};
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

pub fn sign_transfer(hash: &str, private_key_bytes: &[u8]) -> Vec<u8> {
    let signing_key = SigningKey::from_bytes(private_key_bytes.try_into().unwrap());
    // On signe le hash de l'atome
    let signature: Signature = signing_key.sign(hash.as_bytes());
    signature.to_bytes().to_vec()
}

pub fn verify_transfer(hash: &str, signature_bytes: &[u8], public_key_bytes: &[u8]) -> bool {
    let verifying_key = VerifyingKey::from_bytes(public_key_bytes.try_into().unwrap()).unwrap();
    let signature = Signature::from_slice(signature_bytes).unwrap();

    // Si la signature est valide, l'atome est authentique
    verifying_key.verify(hash.as_bytes(), &signature).is_ok()
}

pub fn generate_keypair(root_path: &Path) -> Result<(), String> {
    let identity_dir = root_path.join(".lys/identity");
    fs::create_dir_all(&identity_dir).expect("failed to create identity");

    let secret_path = identity_dir.join("secret.key");
    let public_path = identity_dir.join("public.key");

    if secret_path.exists() {
        return Err("An identity already exists for this repository.".to_string());
    }

    // Génération cryptographique
    let mut secret = [0u8; SECRET_KEY_LENGTH];
    let mut csprng = rand::rng();
    csprng.fill(&mut secret);
    let signing_key = SigningKey::from_bytes(&secret);
    let verifying_key = signing_key.verifying_key();

    // Sauvegarde
    let mut file = File::create(secret_path).expect("failed to create secret key");
    file.write_all(&signing_key.to_bytes())
        .map_err(|e| e.to_string())?;

    let mut file_pub = File::create(public_path).expect("failed to create public key");
    file_pub
        .write_all(verifying_key.as_bytes())
        .map_err(|e| e.to_string())?;
    ok("Keys have been successfully generated");
    Ok(())
}
pub fn sign_message(root_path: &Path, message: &str) -> Result<String, String> {
    let secret_path = root_path.join(".lys/identity/secret.key");

    if !secret_path.exists() {
        return Err("Identity key not found. Please run 'lys keygen' first.".to_string());
    }

    // 1. Lecture de la clé
    let mut file = File::open(secret_path).expect("failed to get secret key");
    let mut bytes = [0u8; 32];
    file.read_exact(&mut bytes).expect("failed to read key");

    let signing_key = SigningKey::from_bytes(&bytes);

    // 2. Signature
    let signature: Signature = signing_key.sign(message.as_bytes());

    // 3. Retourne la signature en Hexadécimal
    Ok(hex::encode(signature.to_bytes()))
}

pub fn verify_signature(
    root_path: &Path,
    message: &str,
    signature_hex: &str,
) -> Result<bool, String> {
    let public_path = root_path.join(".lys/identity/public.key");

    // Si on n'a pas la clé publique, on ne peut pas vérifier (logique)
    if !public_path.exists() {
        return Err("Key public key not found in (.lys/identity/public.key)".to_string());
    }

    // 1. Charger la clé publique
    let mut file = File::open(public_path).map_err(|e| e.to_string())?;
    let mut bytes = [0u8; 32];
    file.read_exact(&mut bytes).map_err(|e| e.to_string())?;

    let verifying_key = VerifyingKey::from_bytes(&bytes).expect("bad keys");

    // 2. Decode signature (Hex -> Bytes)
    let signature_bytes =
        hex::decode(signature_hex).map_err(|_| "Invalid hexadecimal format".to_string())?;

    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| "Invalid signature format".to_string())?;

    // 3. Vérification mathématique
    // Est-ce que cette signature prouve que CE hash a été signé par CETTE clé ?
    match verifying_key.verify(message.as_bytes(), &signature) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

pub fn audit(conn: &Connection) -> Result<bool, sqlite::Error> {
    println!();
    // On récupère Hash et Signature
    let query = "SELECT hash, signature FROM commits ORDER BY id ASC";
    let mut stmt = conn.prepare(query)?;

    let root_path = std::env::current_dir().unwrap();
    let mut errors = 0;
    let mut unsigned = 0;
    let mut valid = 0;

    while let Ok(State::Row) = stmt.next() {
        let hash: String = stmt.read(0)?;
        let signature_opt: Option<String> = stmt.read(1).ok(); // Peut être NULL

        if let Some(signature) = signature_opt {
            // Commit signé : on vérifie
            match crate::crypto::verify_signature(&root_path, &hash, &signature) {
                Ok(true) => {
                    // C'est vide, on ne dit rien pour ne pas polluer, ou juste un petit point
                    ok_audit_commit(&hash[0..7]);
                    valid += 1;
                }
                Ok(false) | Err(_) => {
                    ko_audit_commit(&hash[0..7]);
                    errors += 1;
                }
            }
        } else {
            // Commit non signé (vieux commits avant la feature)
            unsigned += 1;
        }
    }
    println!();
    let total = errors + unsigned + valid;
    if errors > 0 {
        println!("{}",format!(
            "Validated ({valid}/{total}) Unsigned ({unsigned}) Errors ({errors}) Total ({total})"
        )
        .as_str());
        println!();
        return Ok(false);
    } else {
        println!("{}",format!(
            "Validated ({valid}/{total}) Unsigned ({unsigned}) Errors ({errors}) Total ({total})"
        )
        .as_str());
    }
    println!();
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_keygen_sign_verify() {
        let dir = tempdir().unwrap();
        let root_path = dir.path();

        // 1. Generate keypair
        generate_keypair(root_path).expect("Failed to generate keypair");

        let secret_path = root_path.join(".lys/identity/secret.key");
        let public_path = root_path.join(".lys/identity/public.key");

        assert!(secret_path.exists());
        assert!(public_path.exists());

        // 2. Sign message
        let message = "Hello, world!";
        let signature_hex = sign_message(root_path, message).expect("Failed to sign message");

        // 3. Verify signature
        let is_valid = verify_signature(root_path, message, &signature_hex)
            .expect("Failed to verify signature");
        assert!(is_valid);

        // 4. Verify with wrong message
        let is_valid_wrong = verify_signature(root_path, "Wrong message", &signature_hex)
            .expect("Failed to verify signature");
        assert!(!is_valid_wrong);
    }
}
