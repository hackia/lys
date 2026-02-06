use crate::utils::ok;
use sqlite::{Connection, Error, State};
use tabled::{Table, Tabled};

#[derive(Tabled)]
pub struct TodoItem {
    #[tabled(rename = "ID")]
    pub id: i64,
    #[tabled(rename = "Title")]
    pub title: String,
    #[tabled(rename = "Status")]
    pub status: String,
    #[tabled(rename = "Assigned to")]
    pub assigned_to: String,
    #[tabled(rename = "Due date")]
    pub due_date: String,
}

pub fn start_todo(conn: &Connection, id: i64) -> Result<(), Error> {
    let query = "UPDATE todos SET status = 'IN_PROGRESS' WHERE id = ?";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, id))?;
    stmt.next()?;
    ok(format!("Task #{id} is now in progress").as_str());
    Ok(())
}
pub fn add_todo(
    conn: &Connection,
    title: &str,
    assigned_to: Option<&str>,
    due_date: Option<&str>,
) -> Result<(), Error> {
    let query = "INSERT INTO todos (title, assigned_to, due_date) VALUES (?, ?, ?)";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, title))?;
    stmt.bind((2, assigned_to.unwrap_or("Me")))?;
    stmt.bind((3, due_date))?;
    stmt.next()?;
    ok(format!(
        "Todo appended : {title} (due date : {})",
        due_date.unwrap_or("unknown")
    )
    .as_str());
    Ok(())
}

pub fn list_todos(conn: &Connection) -> Result<(), Error> {
    // On récupère les colonnes, en gérant les NULL potentiels avec des valeurs par défaut
    let query = "SELECT id, title, status, IFNULL(assigned_to, 'None'), IFNULL(due_date, 'No limit') FROM todos WHERE status != 'DONE' ORDER BY created_at DESC";
    let mut stmt = conn.prepare(query)?;
    let mut todos = Vec::new();

    while let Ok(State::Row) = stmt.next() {
        todos.push(TodoItem {
            id: stmt.read(0)?,
            title: stmt.read(1)?,
            status: stmt.read(2)?,
            assigned_to: stmt.read(3)?,
            due_date: stmt.read(4)?,
        });
    }

    if todos.is_empty() {
        ok("No pending tasks. You're all caught up!");
    } else {
        println!("{}", Table::new(todos));
    }
    Ok(())
}

pub fn complete_todo(conn: &Connection, id: i64) -> Result<(), Error> {
    let query = "UPDATE todos SET status = 'DONE' WHERE id = ?";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, id))?;
    stmt.next()?;
    ok(format!("Task #{id} done !").as_str());
    Ok(())
}
