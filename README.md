# Silex

## Installation

```bash
git clone https://github.com/hackia/lys.git && cd lys && cargo install --path .
```


## help

```
A secure local first vcs

Usage: lys [COMMAND]

Commands:
  init      Initialize current directory
  new       create a new silex project
  status    Show changes in working directory
  tree      Show repository
  keygen    Generate Ed25519 identity keys for signing commits
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
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version

```

## tree

```

[ DIR ]  d rwx r-x r-x 2026-02-03 07:16 2026-02-03 11:08          - ├── shells
[ SHL ]  f rw- r-- r-- 2026-02-03 07:16 2026-02-03 07:16     3.2 KB │   └── lys.fish
[ DIR ]  d rwx r-x r-x 2026-02-03 07:16 2026-02-03 11:03          - ├── src
[ SRC ]  f rw- r-- r-- 2026-02-03 08:12 2026-02-03 08:12     2.1 KB │   ├── chat.rs
[ SRC ]  f rw- r-- r-- 2026-02-03 07:16 2026-02-03 07:16     7.0 KB │   ├── commit.rs
[ SRC ]  f rw- r-- r-- 2026-02-03 08:47 2026-02-03 08:47     5.0 KB │   ├── crypto.rs
[ SRC ]  f rw- r-- r-- 2026-02-03 08:49 2026-02-03 08:49    10.9 KB │   ├── db.rs
[ SRC ]  f rw- r-- r-- 2026-02-03 08:16 2026-02-03 08:16     4.5 KB │   ├── git.rs
[ SRC ]  f rw- r-- r-- 2026-02-03 08:18 2026-02-03 08:18    21.1 KB │   ├── main.rs
[ SRC ]  f rw- r-- r-- 2026-02-03 07:16 2026-02-03 07:16     1.9 KB │   ├── todo.rs
[ SRC ]  f rw- r-- r-- 2026-02-03 11:03 2026-02-03 11:03    12.3 KB │   ├── tree.rs
[ SRC ]  f rw- r-- r-- 2026-02-03 08:41 2026-02-03 08:41     3.7 KB │   ├── utils.rs
[ SRC ]  f rw- r-- r-- 2026-02-03 08:29 2026-02-03 08:29    35.7 KB │   ├── vcs.rs
[ SRC ]  f rw- r-- r-- 2026-02-03 08:29 2026-02-03 08:29     5.8 KB │   └── web.rs
[ LCK ]  f rw- r-- r-- 2026-02-03 07:16 2026-02-03 07:31    64.4 KB ├── Cargo.lock
[ CFG ]  f rw- r-- r-- 2026-02-03 07:30 2026-02-03 07:30    780.0 B ├── Cargo.toml
[ LIC ]  f rw- r-- r-- 2026-02-03 07:16 2026-02-03 07:16    33.7 KB ├── LICENSE
[ DOC ]  f rw- r-- r-- 2026-02-03 11:07 2026-02-03 11:07     2.9 KB ├── README.md
[ IGN ]  f rw- r-- r-- 2026-02-03 07:16 2026-02-03 07:16      8.0 B └── syl

Summary: 2 directories, 17 files
  
```
