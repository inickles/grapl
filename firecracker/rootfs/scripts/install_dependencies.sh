#!/usr/bin/env bash

set -euo pipefail
set -o xtrace

########################################
# Install dependencies (currently, just debootstrap)
########################################
sudo apt update

sudo apt install --yes --no-install-recommends \
    debootstrap
