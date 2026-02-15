use chrono::{Duration, Local, Timelike};
use sqlite::{Connection, Error, State};

use crate::utils::ok;

// Structure simple pour afficher les messages
pub struct Message {
    pub id: i64,
    pub sender: String,
    pub content: String,
    pub created_at: String,
    pub expires_at: String,
}

/// Envoie un message qui expirera au prochain "20h00"
pub fn send_message(conn: &Connection, sender: &str, content: &str) -> Result<(), Error> {
    // 1. Calcul de la date d'expiration
    let now = Local::now();
    let today_8pm = now
        .date_naive()
        .and_hms_opt(20, 0, 0)
        .unwrap()
        .and_local_timezone(Local)
        .unwrap();

    let expires_at = if now.hour() >= 20 {
        // Si il est 21h, ça expire demain à 20h
        today_8pm + Duration::days(1)
    } else {
        // Sinon, ça expire ce soir à 20h
        today_8pm
    };

    // 2. Insertion en base
    let query = "INSERT INTO ephemeral_messages (sender, content, expires_at) VALUES (?, ?, ?)";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, sender))?;
    stmt.bind((2, content))?;
    stmt.bind((3, expires_at.to_rfc3339().as_str()))?;

    stmt.next()?;
    ok(format!("message expire at : {}", expires_at.format("%d/%m %H:%M")).as_str());
    Ok(())
}

fn cleanup_messages(conn: &Connection) -> Result<(), Error> {
    let query = "DELETE FROM ephemeral_messages WHERE datetime(expires_at) <= datetime('now')";
    conn.execute(query)?;
    Ok(())
}

/// Affiche les messages valides et nettoie les vieux
pub fn list_messages(conn: &Connection) -> Result<Vec<Message>, Error> {
    // D'abord, on nettoie (Garbage Collection)
    cleanup_messages(conn)?;

    let query = "SELECT id, sender, content, created_at, expires_at FROM ephemeral_messages ORDER BY created_at DESC";
    let mut stmt = conn.prepare(query)?;

    let mut messages = Vec::new();

    while let Ok(State::Row) = stmt.next() {
        messages.push(Message {
            id: stmt.read(0)?,
            sender: stmt.read(1)?,
            content: stmt.read(2)?,
            created_at: stmt.read(3)?,
            expires_at: stmt.read(4)?,
        });
    }
    Ok(messages)
}
