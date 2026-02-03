# Silex

## Installation

```bash
git clone https://github.com/hackia/silex.git && cd silex && cargo install --path .
```

```
An new vcs

Usage: silex [COMMAND]

Commands:
  init      Initialize current directory
  new       Create a new silex project
  status    Show changes in working directory
  tree      Show repository
  keygen    Generate Ed25519 identity keys for signing commits
  audit     Verify integrity of commit signatures
  log       Show commit logs
  diff      Show changes between working tree and last commit
  clone     Clone a Git repository into a new Silex directory
  health    Check the source code
  todo      Manage project tasks
  commit    Record changes to the repository
  restore   Discard changes in working directory
  chat      Chat
  sync      Backup repository to a destination (USB, Drive...)
  branch    Create a new branch
  checkout  Switch branches or restore working tree files
  feat      Manage feature branches
  hotfix    Manage hotfix branches
  tag       Manage version tags
  web       Start the web interface
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

```

[ TXT ]  d rwx rwx r-x   2026-01-31 19:23 2026-01-31 19:23          - ├── shells
[ SHL ]  f rw- rw- r--   2026-01-31 19:23 2026-01-31 19:23     3.2 KB │   └── silex.fish
[ TXT ]  d rwx rwx r-x   2026-01-31 19:23 2026-02-03 06:43          - ├── src
[ SRC ]  f rw- rw- r--   2026-01-31 19:23 2026-01-31 19:23     2.1 KB │   ├── chat.rs
[ SRC ]  f rw- r-- r--   2026-02-03 05:15 2026-02-03 05:15     7.0 KB │   ├── commit.rs
[ SRC ]  f rw- rw- r--   2026-01-31 19:23 2026-01-31 19:23     5.2 KB │   ├── crypto.rs
[ SRC ]  f rw- rw- r--   2026-01-31 19:23 2026-01-31 19:23    10.3 KB │   ├── db.rs
[ SRC ]  f rw- rw- r--   2026-02-03 05:48 2026-02-03 05:48     4.5 KB │   ├── git.rs
[ SRC ]  f rw- rw- r--   2026-02-03 06:27 2026-02-03 06:27    21.1 KB │   ├── main.rs
[ SRC ]  f rw- rw- r--   2026-01-31 19:23 2026-01-31 19:23     1.9 KB │   ├── todo.rs
[ SRC ]  f rw- r-- r--   2026-02-03 06:43 2026-02-03 06:43    12.2 KB │   ├── tree.rs
[ SRC ]  f rw- rw- r--   2026-02-02 15:05 2026-02-02 15:05     3.1 KB │   ├── utils.rs
[ SRC ]  f rw- rw- r--   2026-02-02 15:54 2026-02-02 15:54    35.7 KB │   ├── vcs.rs
[ SRC ]  f rw- rw- r--   2026-01-31 19:23 2026-01-31 19:23     5.8 KB │   └── web.rs
[ LCK ]  f rw- rw- r--   2026-01-31 19:23 2026-02-02 14:06    64.4 KB ├── Cargo.lock
[ CFG ]  f rw- r-- r--   2026-02-02 13:53 2026-02-02 13:53    741.0 B ├── Cargo.toml
[ LIC ]  f rw- rw- r--   2026-01-31 19:23 2026-01-31 19:23    33.7 KB ├── LICENSE
[ DOC ]  f rw- r-- r--   2026-02-03 06:02 2026-02-03 06:02     1.2 KB ├── README.md
[ TXT ]  f rw- r-- r--   2026-02-02 14:08 2026-02-02 14:08      8.0 B └── silexium

Summary: 2 directories, 17 files

```
