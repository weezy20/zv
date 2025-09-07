# zv cleanup script for Nushell

hide-env ZV_DIR
$env.PATH = ($env.PATH | split row (char esep) | where $it != "{zv_bin_path}")

print "zv environment cleaned up"
