# Lys

<img src="https://raw.githubusercontent.com/hackia/lys/refs/heads/main/lys.svg" alt="Lys Logo" width="250" align="right" />

Lys is a season Unix-first, **secure, local-first Version Control System** built for privacy, performance, and modern
developer workflows. Written in Rust, it combines robust versioning with integrated tools for team collaboration.

Secure by design means every commit is cryptographically signed using **Ed25519** identity keys.
This prevents history tampering and unauthorized commit modification.
Lys uses a Merkle-tree-based storage format to efficiently store large files and history,
enabling fast synchronization across multiple devices.

Blake3 is the default hash function for file integrity verification.
Todos, team chat, and a polished TUI are included.
Virtual filesystem mounting and remote synchronization make it easy to work on projects from anywhere.
The web interface provides a dashboard for visualizing history and managing tasks.

The CLI provides a simple, intuitive interface for managing your project.
It includes a built-in interactive shell for quick access to common commands.

The TUI, `syl`, is a terminal user interface for managing your project.
It includes features for browsing repository history, managing tasks, and collaborating with team members.
Lys aims to make it easy to get started with a secure, private Git-like workflow.
It is designed for developers who want to work from anywhere and provides a seamless transition from Git to Lys.

Modern workflows include branching, tagging, and merging.

No staging area, no index file, just commits.

SQLite is used as the database backend. 100% Rust. Fast. Secure.



## Key Features

- **Secure by Design**: Every commit is cryptographically signed using **Ed25519** identity keys.
- **Git Integration**: Seamlessly `import` or `clone` existing Git repositories into Lys.
- **Modern Workflow**: Built-in support for `feat` and `hotfix` branches following best practices for software delivery.
- **Integrated Tools**:
  - **Interactive Shell**: Type `lys` without arguments to enter a specialized REPL with history and tab-completion.
  - **Todo Manager**: Integrated task tracking with due dates and assignments.
  - **Team Chat**: Local communication for your team without leaving the terminal.
  - **Beautiful TUI**: Includes `syl`, a comprehensive terminal UI for repository management.
- **Advanced Web Interface**:
  - **Visualization**: Browse repository history, tree structure, and high-quality diffs.
  - **Web Terminal**: Multi-tab support, window splits, and persistent sessions.
  - **Personalization**: Custom titles, banners (images or YouTube), and music (Spotify/YouTube Music).
- **Virtual Filesystem**: Mount specific versions or branches to your filesystem for easy browsing.
- **Decentralized**: Sync with "Remote Architects" or physical destinations like USB drives for air-gapped backups.

## Installation

```bash
cargo install lys
```

### From Source

```bash
git clone https://github.com/hackia/lys.git
cd lys
cargo install --path .
```

This will install both the `lys` CLI and the `syl` TUI.

## Quick Start

```bash
# Initialize a new project interactively
lys new

# Or initialize the current directory
lys init

# Record your first changes
lys commit

# Launch the TUI for a visual overview
syl

# Start the web interface on port 3000
lys web --port 3000
```

## CLI Usage

### Core Commands

- `init` / `new`: Initialize or create projects (with language templates).
- `status` / `log` / `diff`: Inspect current state and history.
- `commit`: Record changes (requires non-empty `syl` message).
- `branch` / `checkout`: Manage and switch between branches.
- `feat` / `hotfix`: Specialized branch management for features and fixes.
- `tag`: Manage version labels.

### Advanced Tools

- `todo`: Manage tasks (`lys todo add`, `lys todo list`, `lys todo close`).
- `chat`: Team communication (`lys chat send "Hello"`, `lys chat list`).
- `web`: Start and configure the web dashboard.
- `mount` / `shell`: Interact with the repository as a virtual filesystem.
- `sync`: Backup to physical destinations.
- `push` / `pull`: Remote synchronization with Architects.

### Security

- `keygen`: Generate your Ed25519 identity keys.
- `audit`: Verify the integrity of all commit signatures in history.
- `verify`: Check repository integrity and missing data blobs.

## Project Structure

```text
.
├── src/
│   ├── main.rs        # Core CLI and command dispatcher
│   ├── bin/
│   │   └── syl.rs     # TUI application
│   ├── vcs.rs         # Version control engine (Merkle-tree based)
│   ├── crypto.rs      # Ed25519 signing and auditing
│   ├── web.rs         # Axum-based web interface and terminal
│   ├── shell.rs       # Interactive REPL implementation
│   ├── todo.rs        # Integrated task management
│   ├── chat.rs        # Team chat logic
│   └── mount.rs       # FUSE-based filesystem mounting
├── Cargo.toml         # Manifest and dependencies
└── README.md          # Documentation
```

## License

This project is licensed under the terms of the AGPL-3.0 file included in the repository.
