# Lys

[![Rust](https://github.com/hackia/lys/actions/workflows/rust.yml/badge.svg)](https://github.com/hackia/lys/actions/workflows/rust.yml)


<img src="https://github.com/hackia/lys/blob/main/lys.svg" alt="Lys Logo" width="250" align="right"/>

Lys is a **secure, local-first Version Control System (VCS)** designed for privacy, performance, and modern developer
workflows. Written in Rust, it combines robust versioning with integrated tools for team collaboration.

## Key Features

- **Secure by Design**: Ed25519 identity keys for cryptographically signing every commit.
- **Git Integration**: Seamlessly `import` or `clone` existing Git repositories into Lys.
- **Modern Workflow**: Built-in support for `feat`, `hotfix`, and `tag` management.
- **Integrated Tools**:
    - **Interactive Shell**: Type `lys` without arguments to enter an interactive shell with history (`~/.lys-history`).
    - **Todo Manager**: Track project tasks directly within the VCS.
    - **Team Chat**: Communicate with your team without leaving your terminal.
    - **Advanced Web Interface**:
        - Visualize your repository, commits, and diffs.
        - **Integrated Terminal**: A powerful web-based terminal with multi-tab support, window splits (
          horizontal/vertical), and persistent sessions (tmux/screen style).
        - **Music Integration**: Personalize your dashboard with your favorite albums from **Spotify** or **YouTube
          Music**.
        - **Personalized Banner**: Showcase your project with a **YouTube video** or a **custom image banner** directly
          on the home page.
- **Mounting**: Mount specific versions or the current HEAD to a directory as a virtual filesystem.
- **Decentralized**: `push` and `pull` to remote architects, or `sync` to physical destinations like USB drives.
- **TUI Support**: Includes `syl`, a beautiful terminal user interface for managing your work.

## Installation

```bash
git clone https://github.com/hackia/lys.git
cd lys
cargo install --path .
```

This will install both `lys` (CLI) and `syl` (TUI).

## Quick Start

```bash
# Initialize a new project
lys init

# Add and commit changes
lys commit

# Check status and logs
lys status
lys log

# Launch the TUI
syl
```

## CLI Usage

```text
Usage: lys [COMMAND]

Commands:
  init      Initialize current directory
  new       Create a new lys project
  verify    Check repository integrity and missing blobs
  summary   Show working directory infos
  status    Show changes in working directory
  push      Push local commits to a remote architect
  pull      Pull commits from a remote architect
  prune     Maintain repository health by removing old history
  shell     Open a temporary shell with the code mounted
  mount     Mount a specific version or the current head to a directory
  tree      Show repository structure
  import    Import a Git repository into Lys
  keygen    Generate Ed25519 identity keys for signing commits
  serve     Start the Silex Node (Daemon) to receive atoms
  audit     Verify integrity of commit signatures
  log       Show commit logs
  diff      Show changes between working tree and last commit
  clone     Clone a Git repository into a new lys repository
  health    Check the source code
  todo      Manage project tasks
  commit    Record changes to the repository
  restore   Discard changes in working directory
  chat      Chat with the team
  sync      Backup repository to a destination (USB, Drive...)
  branch    Create a new branch
  checkout  Switch branches or restore working tree files
  feat      Manage feature branches
  hotfix    Manage hotfix branches
  tag       Manage version tags
  web       Start the web interface
  spotify   Set the Music album/track to display on the home page
  video     Set the YouTube video banner to display on the home page
  banner    Set the image banner to display on the home page
```

## Project Structure

```text
.
├── src/
│   ├── main.rs        # Core CLI logic (lys)
│   ├── bin/
│   │   └── syl.rs     # TUI application (syl)
│   ├── shell.rs       # Interactive REPL shell logic
│   ├── vcs.rs         # Version control engine
│   ├── crypto.rs      # Ed25519 signing and hashing
│   ├── todo.rs        # Task management logic
│   ├── chat.rs        # Team chat implementation
│   ├── mount.rs       # Filesystem mounting logic
│   └── web.rs         # Web interface server
├── Cargo.toml         # Project dependencies
└── README.md          # You are here
```

## License

This project is licensed under the terms of the LICENSE file included in the repository.
