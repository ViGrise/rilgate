#!/bin/sh
# Creates the environment file with a generated signing secret on the first
# installation, so that the service can start after the upstream is configured.
set -e
environment=/etc/rilgate/environment
if [ ! -e "$environment" ]; then
  umask 077
  secret="$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')"
  printf '# Rilgate service environment. /usr/share/doc/rilgate/env.example lists all variables.\nRILGATE_SECURITY_SECRET=%s\n' \
    "$secret" > "$environment"
fi
if command -v systemctl >/dev/null 2>&1; then
  systemctl daemon-reload >/dev/null 2>&1 || true
fi
