complete -c silex -f

complete -c silex -n __fish_use_subcommand -a new -d 'Create a new silex project'
complete -c silex -n __fish_use_subcommand -a status -d 'Show changes in working directory'
complete -c silex -n __fish_use_subcommand -a log -d 'Show commit logs'
complete -c silex -n __fish_use_subcommand -a diff -d 'Show changes between working tree and last commit'
complete -c silex -n __fish_use_subcommand -a restore -d 'Discard changes in working directory'
complete -c silex -n __fish_use_subcommand -a commit -d 'Record changes to the repository'
complete -c silex -n __fish_use_subcommand -a sync -d 'Backup repository to a destination'
complete -c silex -n __fish_use_subcommand -a branch -d 'Create a new branch'
complete -c silex -n __fish_use_subcommand -a checkout -d 'Switch branches or restore working tree files'
complete -c silex -n __fish_use_subcommand -a feat -d 'Manage feature branches'
complete -c silex -n __fish_use_subcommand -a hotfix -d 'Manage hotfix branches'
complete -c silex -n __fish_use_subcommand -a tag -d 'Manage version tags'
complete -c silex -n __fish_use_subcommand -a web -d 'Start the web interface'
complete -c silex -n __fish_use_subcommand -a tree -d 'Print the repository tree interface'
complete -c silex -n __fish_use_subcommand -a keygen -d 'Generate Ed25519 identity keys for signing commits'
complete -c silex -n __fish_use_subcommand -a audit -d 'Verify all commits signatures'
complete -c silex -n __fish_use_subcommand -a health -d 'Check source code'
complete -c silex -n __fish_use_subcommand -a clone -d 'Clone a silex or a git repository'
complete -c silex -n __fish_use_subcommand -a init -d 'Initialize current directory'
complete -c silex -n "__fish_seen_subcommand_from commit" -s m -l message -r -d 'Description of the changes'

complete -c silex -n "__fish_seen_subcommand_from restore" -F

complete -c silex -n "__fish_seen_subcommand_from sync" -F

complete -c silex -n "__fish_seen_subcommand_from web" -s p -l port -x -d 'Port (default 3000)'

complete -c silex -n "__fish_seen_subcommand_from feat; and not __fish_seen_subcommand_from start finish" -a "start finish"

complete -c silex -n "__fish_seen_subcommand_from hotfix; and not __fish_seen_subcommand_from start finish" -a "start finish"

complete -c silex -n "__fish_seen_subcommand_from tag; and not __fish_seen_subcommand_from create list" -a "create list"
complete -c silex -n "__fish_seen_subcommand_from tag; and __fish_seen_subcommand_from create" -s m -l message -r -d Description

complete -c silex -s h -l help -d 'Print a short help text and exit'
complete -c silex -n __fish_use_subcommand -a chat -d 'Internal messaging system'
complete -c silex -n "__fish_seen_subcommand_from chat; and not __fish_seen_subcommand_from send list" -a "send list"

complete -c silex -n __fish_use_subcommand -a todo -d 'Manage project tasks'

complete -c silex -n "__fish_seen_subcommand_from todo; and not __fish_seen_subcommand_from add list close" -a "add list close"

complete -c silex -n "__fish_seen_subcommand_from todo; and __fish_seen_subcommand_from add" -s u -d 'Assign to user'
complete -c silex -n "__fish_seen_subcommand_from todo; and __fish_seen_subcommand_from add" -s d -l due -d 'Due date (YYYY-MM-DD)'
