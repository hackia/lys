use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn verify_proofs(payload_hash: &str, tsa_proof: &[u8], ots_proof: &[u8]) -> Result<()> {
    if env::var("SILEXIUM_SKIP_PROOF_VERIFY").ok().as_deref() == Some("1") {
        return Ok(());
    }

    let tsa_cmd = env::var("SILEXIUM_TSA_VERIFY")
        .map_err(|_| anyhow!("SILEXIUM_TSA_VERIFY is required for TSA verification"))?;
    let ots_cmd = env::var("SILEXIUM_OTS_VERIFY")
        .map_err(|_| anyhow!("SILEXIUM_OTS_VERIFY is required for OTS verification"))?;

    run_verify(&tsa_cmd, payload_hash, tsa_proof, "tsa")?;
    run_verify(&ots_cmd, payload_hash, ots_proof, "ots")?;
    Ok(())
}

fn run_verify(cmd: &str, payload_hash: &str, proof: &[u8], kind: &str) -> Result<()> {
    let proof_path = write_temp(kind, proof)?;
    let output = Command::new(cmd)
        .arg(payload_hash)
        .arg(&proof_path)
        .output()
        .with_context(|| format!("failed to run {kind} verifier"))?;

    let _ = fs::remove_file(&proof_path);

    if !output.status.success() {
        return Err(anyhow!(
            "{kind} verification failed: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

fn write_temp(kind: &str, proof: &[u8]) -> Result<PathBuf> {
    let mut path = env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    path.push(format!("silexium-{kind}-{nanos}.proof"));
    fs::write(&path, proof).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::verify_proofs;
    use std::env;
    use std::fs;

    #[test]
    fn verify_real_proofs_env() {
        if env::var("SILEXIUM_SKIP_PROOF_VERIFY").ok().as_deref() == Some("1") {
            eprintln!("skipping: SILEXIUM_SKIP_PROOF_VERIFY=1");
            return;
        }

        let payload_hash = match env::var("SILEXIUM_PROOF_PAYLOAD_HASH") {
            Ok(val) if !val.trim().is_empty() => val,
            _ => {
                eprintln!("skipping: SILEXIUM_PROOF_PAYLOAD_HASH not set");
                return;
            }
        };
        let tsa_path = match env::var("SILEXIUM_TSA_PROOF_FILE") {
            Ok(val) if !val.trim().is_empty() => val,
            _ => {
                eprintln!("skipping: SILEXIUM_TSA_PROOF_FILE not set");
                return;
            }
        };
        let ots_path = match env::var("SILEXIUM_OTS_PROOF_FILE") {
            Ok(val) if !val.trim().is_empty() => val,
            _ => {
                eprintln!("skipping: SILEXIUM_OTS_PROOF_FILE not set");
                return;
            }
        };

        if env::var("SILEXIUM_TSA_VERIFY").ok().as_deref().unwrap_or("").is_empty() {
            eprintln!("skipping: SILEXIUM_TSA_VERIFY not set");
            return;
        }
        if env::var("SILEXIUM_OTS_VERIFY").ok().as_deref().unwrap_or("").is_empty() {
            eprintln!("skipping: SILEXIUM_OTS_VERIFY not set");
            return;
        }

        let tsa = fs::read(&tsa_path).expect("read TSA proof");
        let ots = fs::read(&ots_path).expect("read OTS proof");
        assert!(!tsa.is_empty(), "tsa proof is empty");
        assert!(!ots.is_empty(), "ots proof is empty");

        verify_proofs(&payload_hash, &tsa, &ots).expect("proof verification failed");
    }
}
