# Lys

[![Rust](https://github.com/hackia/lys/actions/workflows/rust.yml/badge.svg)](https://github.com/hackia/lys/actions/workflows/rust.yml)

## Installation

```bash
git clone https://github.com/hackia/lys.git && cd lys && cargo install --path .
```


## help

```
A secure local first vcs

Usage: lys [COMMAND]

Commands:
  doctor    Check system health and permissions for lys
  init      Initialize current directory
  new       Create a new lys project
  verify    Check repository integrity and missing blobs
  summary   Show working directory infos
  status    Show changes in working directory
  push      Push local commits to a remote architect
  pull      Pull commits from a remote architect
  prune     Maintain repository health by removing old history and reclaiming disk space.
  shell     Open a temporary shell with the code mounted
  mount     Mount a specific version or the current head to a directory
  tree      Show repository
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
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version

```

## tree
 
```
m                                                                        [ 5175fa51cbe248ee2ae0cb3553f1276fa77626ac ]
d [ 53653f5 ] ├── .github         
d [ 2c3fa72 ] │   └── workflows      
f [ f456e60 ] │       └── ci.yml    
f [ df0c8e4 ] ├── .gitignore    
f [ eea3c2f ] ├── CHANGELOG.md    
f [ 2815c92 ] ├── CMakeLists.txt  
f [ 365b998 ] ├── DOCUMENTATION_INDEX.md
f [ ddf722d ] ├── DONE.md
f [ 9d5569c ] ├── DSL_PARSER_COMPLETE.md
f [ 0344d03 ] ├── EXECUTIVE_SUMMARY_FR.md
f [ 69727a8 ] ├── HEARTBEAT_IMPLEMENTATION.md
f [ eb52fec ] ├── IMPLEMENTATION_COMPLETE.md
f [ ab0b6d4 ] ├── IMPLEMENTATION_COMPLETE_V1_1.md
f [ 23e857c ] ├── IMPLEMENTATION_SUMMARY.md
f [ 5915a06 ] ├── INDEX.md
f [ 42b9965 ] ├── LICENSE
f [ 8bd7eb6 ] ├── PRIORITIES_1_TO_6_COMPLETE.md
f [ 2e6b09e ] ├── QUICK_START.md  
f [ 6238622 ] ├── README.md       
f [ 95224ae ] ├── RELEASE_NOTES_V1_0.md
f [ c822cf9 ] ├── RESOURCE_LIMITS_DELIVERY.md
f [ 3952210 ] ├── RESOURCE_LIMITS_IMPLEMENTATION.md
f [ 2cebec7 ] ├── RESOURCE_LIMITS_QUICKSTART.sh
f [ 6ba7b2a ] ├── SESSION_COMPLETE.md
f [ 23cc497 ] ├── SESSION_SUMMARY.md
f [ e2431d0 ] ├── SSH_IMPLEMENTATION.md
f [ 8c68644 ] ├── V1.0_ROADMAP.md
f [ c65f180 ] ├── V1_0_FILE_CHANGES.md
d [ 5edb6a4 ] ├── apps            
d [ 1742aaa ] │   └── hamon       
f [ a15b969 ] │       └── main.cpp
d [ 20cf72a ] ├── examples        
f [ 922eef3 ] │   ├── resource_limits_example.hc
f [ 95ae4b0 ] │   ├── ssh_example.cpp
f [ 1dddff5 ] │   └── test_resource_limits.sh
f [ be279bd ] ├── hamon.txt
f [ 523d09a ] ├── hamon_ssh.hc    
d [ 63a7b6d ] ├── help            
f [ a2ec761 ] │   ├── API_REFERENCE.md
f [ c18b27a ] │   ├── ARCHITECTURE.md
f [ a7d8556 ] │   ├── DSL_REFERENCE.md
f [ accdaa8 ] │   ├── GETTING_STARTED.md
f [ 9d82a35 ] │   ├── INDEX.md        
f [ a50ea76 ] │   ├── RESOURCE_LIMITS.md
f [ 96d5b4a ] │   ├── SSH_GUIDE.md
f [ d7428ce ] │   └── USER_GUIDE.md
d [ 32d013f ] ├── include
f [ 9d17c36 ] │   ├── GenericTaskExecutor.hpp
f [ 7070c4b ] │   ├── Hamon.hpp
f [ 5727e55 ] │   ├── HamonCube.hpp
f [ 9fd502a ] │   ├── HamonNode.hpp
f [ 2464ef0 ] │   ├── Heartbeat.hpp
f [ 5768499 ] │   ├── Make.hpp     
f [ 278b87f ] │   ├── RemoteNodeLauncher.hpp
f [ f718fbc ] │   ├── SSHExecutor.hpp
f [ 25921cc ] │   └── helpers.hpp    
f [ ee1149c ] ├── input.txt
f [ 710a3d2 ] ├── make.hc  
f [ a01e9f5 ] ├── make_r710_2cpu.hc
d [ de646cf ] ├── src             
f [ 723a35b ] │   ├── GenericTaskExecutor.cpp
f [ d09131a ] │   ├── Hamon.cpp
f [ b22535a ] │   ├── HamonCube.cpp
f [ 056ea5b ] │   ├── HamonNode.cpp
f [ a8c8099 ] │   ├── Heartbeat.cpp
f [ 676bc35 ] │   ├── Make.cpp
f [ 2c53ae1 ] │   ├── RemoteNodeLauncher.cpp
f [ 5995d41 ] │   ├── SSHExecutor.cpp
f [ 4ed90f2 ] │   └── helpers.cpp
f [ 2829e48 ] ├── test_build.hc 
d [ 988b989 ] └── tests
f [ bbdab08 ]     ├── test_hamon.cpp
f [ f72c11d ]     ├── test_hamon_cube.cpp
f [ 6d00caa ]     ├── test_hamon_node.cpp
f [ df184da ]     └── test_hamon_ring.cpp

```
