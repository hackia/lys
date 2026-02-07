use crossterm::event::{Event, KeyCode, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::Stylize;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Paragraph};
use std::io::Result;

pub struct App {
    pub should_quit: bool,
    pub help_mode: bool,
    pub editor_mode: bool,
    pub web_mode: bool,
    pub commit_mode: bool,
    pub todo_mode: bool,
    pub chat_mode: bool,
    pub files_mode: bool,
    pub timeline_mode: bool,
    pub logs_mode: bool,
    pub shell_mode: bool,
    pub health_mode: bool,
}

fn help(f: &mut Frame) {
    let text = Text::from(vec![
        Line::from("F1   => Show help".white()),
        Line::from("F2   => Launch editor".white()),
        Line::from("F3   => Search on web".white()),
        Line::from("F4   => Exit syl".white()),
        Line::from("F5   => Commit".white()),
        Line::from("F6   => Manage todos".white()),
        Line::from("F7   => Chat".white()),
        Line::from("F8   => Show tree".white()),
        Line::from("F9   => Show timeline".white()),
        Line::from("F10  => Show logs".white()),
        Line::from("F11  => Shell".white()),
        Line::from("F12  => Check the code's health".white()),
    ]);
    f.render_widget(
        Paragraph::new(text).block(
            Block::default()
                .title_top(" Help ")
                .title_alignment(Alignment::Center)
                .borders(Borders::all()),
        ),
        f.area(),
    );
}

fn editor(f: &mut Frame) {
    f.render_widget(
        Paragraph::default().block(
            Block::default()
                .title_top(" Editor ")
                .title_alignment(Alignment::Center)
                .borders(Borders::all()),
        ),
        f.area(),
    );
}

fn web(f: &mut Frame) {
    f.render_widget(
        Paragraph::default().block(
            Block::default()
                .title_top(" Web ")
                .title_alignment(Alignment::Center)
                .borders(Borders::all()),
        ),
        f.area(),
    );
}

fn commit(f: &mut Frame) {
    f.render_widget(
        Paragraph::default().block(
            Block::default()
                .title_top(" Commit ")
                .title_alignment(Alignment::Center)
                .borders(Borders::all()),
        ),
        f.area(),
    );
}

fn tree(f: &mut Frame) {
    f.render_widget(
        Paragraph::default().block(
            Block::default()
                .title_top(" Tree ")
                .title_alignment(Alignment::Center)
                .borders(Borders::all()),
        ),
        f.area(),
    );
}

fn todos(f: &mut Frame) {
    f.render_widget(
        Paragraph::default().block(
            Block::default()
                .title_top(" Todos ")
                .title_alignment(Alignment::Center)
                .borders(Borders::all()),
        ),
        f.area(),
    );
}

fn timeline(f: &mut Frame) {
    f.render_widget(
        Paragraph::default().block(
            Block::default()
                .title_top(" Timeline ")
                .title_alignment(Alignment::Center)
                .borders(Borders::all()),
        ),
        f.area(),
    );
}

fn health(f: &mut Frame) {
    f.render_widget(
        Paragraph::default().block(
            Block::default()
                .title_top(" Health ")
                .title_alignment(Alignment::Center)
                .borders(Borders::all()),
        ),
        f.area(),
    );
}

fn logs(f: &mut Frame) {
    f.render_widget(
        Paragraph::default().block(
            Block::default()
                .title_top(" Logs ")
                .title_alignment(Alignment::Center)
                .borders(Borders::all()),
        ),
        f.area(),
    );
}

fn shell(f: &mut Frame) {
    f.render_widget(
        Paragraph::default().block(
            Block::default()
                .title_top(" Shell ")
                .title_alignment(Alignment::Center)
                .borders(Borders::all()),
        ),
        f.area(),
    );
}

fn chat(f: &mut Frame) {
    f.render_widget(
        Paragraph::default().block(
            Block::default()
                .title_top(" Chat ")
                .title_alignment(Alignment::Center)
                .borders(Borders::all()),
        ),
        f.area(),
    );
}

fn ui(f: &mut Frame) {
    f.render_widget(
        Paragraph::default().block(Block::default().borders(Borders::all())),
        f.area(),
    );
}

fn main() -> Result<()> {
    let mut terminal = ratatui::init();

    let mut app = App {
        should_quit: false,
        help_mode: false,
        editor_mode: false,
        web_mode: false,
        commit_mode: false,
        todo_mode: false,
        chat_mode: false,
        files_mode: false,
        timeline_mode: false,
        logs_mode: false,
        shell_mode: false,
        health_mode: false,
    };
    loop {
        if app.should_quit {
            break;
        }
        assert!(
            terminal
                .draw(|f| {
                    if app.help_mode {
                        help(f);
                    } else if app.editor_mode {
                        editor(f);
                    } else if app.web_mode {
                        web(f);
                    } else if app.commit_mode {
                        commit(f);
                    } else if app.todo_mode {
                        todos(f);
                    } else if app.timeline_mode {
                        timeline(f);
                    } else if app.health_mode {
                        health(f);
                    } else if app.chat_mode {
                        chat(f);
                    } else if app.files_mode {
                        tree(f);
                    } else if app.shell_mode {
                        shell(f);
                    } else if app.logs_mode {
                        logs(f);
                    } else {
                        ui(f);
                    }
                })
                .is_ok()
        );
        match crossterm::event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::F(1) => {
                    app.should_quit = false;
                    app.help_mode = true;
                    app.editor_mode = false;
                    app.web_mode = false;
                    app.commit_mode = false;
                    app.todo_mode = false;
                    app.chat_mode = false;
                    app.files_mode = false;
                    app.timeline_mode = false;
                    app.logs_mode = false;
                    app.shell_mode = false;
                    app.health_mode = false;
                }
                KeyCode::F(2) => {
                    app.should_quit = false;
                    app.help_mode = false;
                    app.editor_mode = true;
                    app.web_mode = false;
                    app.commit_mode = false;
                    app.todo_mode = false;
                    app.chat_mode = false;
                    app.files_mode = false;
                    app.timeline_mode = false;
                    app.logs_mode = false;
                    app.shell_mode = false;
                    app.health_mode = false;
                }

                KeyCode::F(3) => {
                    app.should_quit = false;
                    app.help_mode = false;
                    app.editor_mode = false;
                    app.web_mode = true;
                    app.commit_mode = false;
                    app.todo_mode = false;
                    app.chat_mode = false;
                    app.files_mode = false;
                    app.timeline_mode = false;
                    app.logs_mode = false;
                    app.shell_mode = false;
                    app.health_mode = false;
                }
                KeyCode::F(4) => {
                    app.should_quit = true;
                    app.help_mode = false;
                    app.editor_mode = false;
                    app.web_mode = false;
                    app.commit_mode = false;
                    app.todo_mode = false;
                    app.chat_mode = false;
                    app.files_mode = false;
                    app.timeline_mode = false;
                    app.logs_mode = false;
                    app.shell_mode = false;
                    app.health_mode = false;
                }
                KeyCode::F(5) => {
                    app.should_quit = false;
                    app.help_mode = false;
                    app.editor_mode = false;
                    app.web_mode = false;
                    app.commit_mode = true;
                    app.todo_mode = false;
                    app.chat_mode = false;
                    app.files_mode = false;
                    app.timeline_mode = false;
                    app.logs_mode = false;
                    app.shell_mode = false;
                    app.health_mode = false;
                }
                KeyCode::F(6) => {
                    app.should_quit = false;
                    app.help_mode = false;
                    app.editor_mode = false;
                    app.web_mode = false;
                    app.commit_mode = false;
                    app.todo_mode = true;
                    app.chat_mode = false;
                    app.files_mode = false;
                    app.timeline_mode = false;
                    app.logs_mode = false;
                    app.shell_mode = false;
                    app.health_mode = false;
                }
                KeyCode::F(7) => {
                    app.should_quit = false;
                    app.help_mode = false;
                    app.editor_mode = false;
                    app.web_mode = false;
                    app.commit_mode = false;
                    app.todo_mode = false;
                    app.chat_mode = true;
                    app.files_mode = false;
                    app.timeline_mode = false;
                    app.logs_mode = false;
                    app.shell_mode = false;
                    app.health_mode = false;
                }
                KeyCode::F(8) => {
                    app.should_quit = false;
                    app.help_mode = false;
                    app.editor_mode = false;
                    app.web_mode = false;
                    app.commit_mode = false;
                    app.todo_mode = false;
                    app.chat_mode = false;
                    app.files_mode = true;
                    app.timeline_mode = false;
                    app.logs_mode = false;
                    app.shell_mode = false;
                    app.health_mode = false;
                }
                KeyCode::F(9) => {
                    app.should_quit = false;
                    app.help_mode = false;
                    app.editor_mode = false;
                    app.web_mode = false;
                    app.commit_mode = false;
                    app.todo_mode = false;
                    app.chat_mode = false;
                    app.files_mode = false;
                    app.timeline_mode = true;
                    app.logs_mode = false;
                    app.shell_mode = false;
                    app.health_mode = false;
                }
                KeyCode::F(10) => {
                    app.should_quit = false;
                    app.help_mode = false;
                    app.editor_mode = false;
                    app.web_mode = false;
                    app.commit_mode = false;
                    app.todo_mode = false;
                    app.chat_mode = false;
                    app.files_mode = false;
                    app.timeline_mode = false;
                    app.logs_mode = true;
                    app.shell_mode = false;
                    app.health_mode = false;
                }
                KeyCode::F(11) => {
                    app.should_quit = false;
                    app.help_mode = false;
                    app.editor_mode = false;
                    app.web_mode = false;
                    app.commit_mode = false;
                    app.todo_mode = false;
                    app.chat_mode = false;
                    app.files_mode = false;
                    app.timeline_mode = false;
                    app.logs_mode = false;
                    app.shell_mode = true;
                    app.health_mode = false;
                }
                KeyCode::F(12) => {
                    app.should_quit = false;
                    app.help_mode = false;
                    app.editor_mode = false;
                    app.web_mode = false;
                    app.commit_mode = false;
                    app.todo_mode = false;
                    app.chat_mode = false;
                    app.files_mode = false;
                    app.timeline_mode = false;
                    app.logs_mode = false;
                    app.shell_mode = false;
                    app.health_mode = true;
                }
                _ => {
                    app.should_quit = false;
                    app.help_mode = false;
                    app.editor_mode = false;
                    app.web_mode = false;
                    app.commit_mode = false;
                    app.todo_mode = false;
                    app.chat_mode = false;
                    app.files_mode = false;
                    app.timeline_mode = false;
                    app.logs_mode = false;
                    app.shell_mode = false;
                    app.health_mode = false;
                }
            },
            _ => {}
        }
    }
    ratatui::restore();
    Ok(())
}
