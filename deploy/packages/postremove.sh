#!/bin/sh
# A Debian purge removes the environment file that the postinstall script created.
set -e
if [ "$1" = "purge" ]; then
  rm -f /etc/rilgate/environment
  rmdir /etc/rilgate 2>/dev/null || true
fi
if command -v systemctl >/dev/null 2>&1; then
  systemctl daemon-reload >/dev/null 2>&1 || true
fi
