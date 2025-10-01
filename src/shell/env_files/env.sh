#!/bin/sh
# zv shell setup
# affix colons on either side of $PATH to simplify matching
{zv_dir_export}
case ":{zv_path_separator}${PATH}{zv_path_separator}" in
    *{zv_path_separator}{zv_bin_path}{zv_path_separator}*)
        ;;
    *)
        # Prepending path in case a system-installed binary needs to be overridden
        export PATH="{zv_bin_path}{zv_path_separator}$PATH"
        ;;
esac
