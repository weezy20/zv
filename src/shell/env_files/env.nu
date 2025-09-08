# zv shell setup for Nushell
$env.ZV_DIR = "{zv_dir}"
$env.PATH = ($env.PATH | split row (char esep) | prepend "{zv_bin_path}" | uniq)
