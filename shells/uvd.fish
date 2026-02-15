complete -c uvd -f

complete -c uvd -n __fish_use_subcommand -a create -d 'Create a signed UVD archive'
complete -c uvd -n __fish_use_subcommand -a verify -d 'Verify a signed UVD archive'
complete -c uvd -n __fish_use_subcommand -a extract -d 'Extract a signed UVD archive'

complete -c uvd -n "__fish_seen_subcommand_from verify" -a "(__fish_complete_suffix .uvd)" -d 'UVD archive'
complete -c uvd -n "__fish_seen_subcommand_from extract" -a "(__fish_complete_suffix .uvd)" -d 'UVD archive'

complete -c uvd -s h -l help -d 'Print help and exit'
complete -c uvd -s V -l version -d 'Print version and exit'
