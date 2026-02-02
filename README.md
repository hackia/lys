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
