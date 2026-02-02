use crate::utils::{ko, ko_audit_commit, ok, ok_audit_commit};
use ed25519_dalek::ed25519::signature::SignerMut;
use ed25519_dalek::{Signature, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use sqlite::{Connection, State};
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
pub fn generate_keypair(root_path: &Path) -> Result<(), String> {
    let identity_dir = root_path.join(".silex/identity");
    fs::create_dir_all(&identity_dir).expect("failed to create identity");

    let secret_path = identity_dir.join("secret.key");
    let public_path = identity_dir.join("public.key");

    if secret_path.exists() {
        return Err("Une identité existe déjà pour ce dépôt.".to_string());
    }

    // Génération cryptographique
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key = signing_key.verifying_key();

    // Sauvegarde
    let mut file = File::create(secret_path).expect("failed to create secret key");
    file.write_all(&signing_key.to_bytes())
        .map_err(|e| e.to_string())?;

    let mut file_pub = File::create(public_path).expect("failed to create public key");
    file_pub
        .write_all(verifying_key.as_bytes())
        .map_err(|e| e.to_string())?;
    ok("Keys has been successfully generated");
    Ok(())
}

// Signe un message (le hash du commit)
pub fn sign_message(root_path: &Path, message: &str) -> Result<String, String> {
    let secret_path = root_path.join(".silex/identity/secret.key");

    if !secret_path.exists() {
        return Err("launch 'sx keygen' first".to_string());
    }

    // 1. Lecture de la clé
    let mut file = File::open(secret_path).expect("failed to get secret key");
    let mut bytes = [0u8; 32];
    file.read_exact(&mut bytes).expect("failed to read key");

    let mut signing_key = SigningKey::from_bytes(&bytes);

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
    let public_path = root_path.join(".silex/identity/public.key");

    // Si on n'a pas la clé publique, on ne peut pas vérifier (logique)
    if !public_path.exists() {
        return Err("Clé publique introuvable (.silex/identity/public.key)".to_string());
    }

    // 1. Charger la clé publique
    let mut file = File::open(public_path).map_err(|e| e.to_string())?;
    let mut bytes = [0u8; 32];
    file.read_exact(&mut bytes).map_err(|e| e.to_string())?;

    let verifying_key = VerifyingKey::from_bytes(&bytes).expect("bad keys");

    // 2. Décoder la signature (Hex -> Bytes)
    let signature_bytes =
        hex::decode(signature_hex).map_err(|_| "Format hexadécimal invalide".to_string())?;

    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| "Format de signature invalide".to_string())?;

    // 3. Vérification mathématique
    // Est-ce que cette signature prouve que CE hash a été signé par CETTE clé ?
    match verifying_key.verify(message.as_bytes(), &signature) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

pub fn audit(conn: &Connection) -> Result<bool, sqlite::Error> {
    println!();
    ok("Auditing commits...\n");

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
                Ok(false) => {
                    ko_audit_commit(&hash[0..7]);
                    errors += 1;
                }
                Err(e) => {
                    ko(format!("[ {} ] audit impossible ( {e} )", &hash[0..7]).as_str());
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
        ko(format!("Audit has been detected {errors} commit's signature errors.").as_str());
        println!();
        ko(format!(
            "Validated ({valid}/{total}) Unsigned ({unsigned}) Errors ({errors}) Total ({total})"
        )
        .as_str());
        println!();
        return Ok(false);
    } else {
        ok("Audit successfull");
        println!();
        ok(format!(
            "Validated ({valid}/{total}) Unsigned ({unsigned}) Errors ({errors}) Total ({total})"
        )
        .as_str());
    }
    println!();
    Ok(true)
}
