complete -c syl -f

complete -c syl -n __fish_use_subcommand -a create -d 'Create a signed UVD archive'
complete -c syl -n __fish_use_subcommand -a verify -d 'Verify a signed UVD archive'
complete -c syl -n __fish_use_subcommand -a extract -d 'Extract a signed UVD archive'

complete -c syl -n "__fish_seen_subcommand_from verify" -a "(__fish_complete_suffix .syl)" -d 'UVD archive'
complete -c syl -n "__fish_seen_subcommand_from extract" -a "(__fish_complete_suffix .syl)" -d 'UVD archive'

complete -c syl -s h -l help -d 'Print help and exit'
complete -c syl -s V -l version -d 'Print version and exit'
