#!/bin/sh
# Stops the service before its files are removed. An upgrade keeps it running.
set -e
case "$1" in
  remove | purge | 0)
    if command -v systemctl >/dev/null 2>&1; then
      systemctl disable --now rilgate.service >/dev/null 2>&1 || true
    fi
    ;;
esac
