# zv shell setup for Nushell
{zv_dir_export}
$env.PATH = ($env.PATH | split row (char esep) | prepend "{zv_bin_path}" | uniq)
