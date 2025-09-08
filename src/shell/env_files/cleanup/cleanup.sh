#!/bin/sh
# zv cleanup script

unset ZV_DIR
case ":$PATH:" in
    *:"{zv_bin_path}":*)
        PATH=$(echo "$PATH" | sed -e "s|{zv_bin_path}:||g" -e "s|:{zv_bin_path}||g")
        export PATH
        ;;
esac

echo "zv environment cleaned up"
