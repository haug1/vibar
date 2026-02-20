#!/usr/bin/env bash
set -euo pipefail

if [[ "${EUID}" -ne 0 ]]; then
  SUDO="sudo"
else
  SUDO=""
fi

if [[ -f /etc/os-release ]]; then
  . /etc/os-release
else
  echo "Could not detect distro (/etc/os-release missing)." >&2
  exit 1
fi

install_arch() {
  ${SUDO} pacman -Sy --needed \
    base-devel \
    pkgconf \
    gtk4 \
    wayland \
    wayland-protocols \
    rustup
}

install_debian() {
  ${SUDO} apt update
  ${SUDO} apt install -y \
    build-essential \
    pkg-config \
    libgtk-4-dev \
    libgtk4-layer-shell-dev \
    libwayland-dev \
    wayland-protocols \
    curl

  if ! command -v rustup >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  fi
}

case "${ID:-}" in
  arch|cachyos|endeavouros|manjaro)
    install_arch
    ;;
  ubuntu|debian|linuxmint|pop)
    install_debian
    ;;
  *)
    if [[ "${ID_LIKE:-}" == *"arch"* ]]; then
      install_arch
    elif [[ "${ID_LIKE:-}" == *"debian"* ]]; then
      install_debian
    else
      echo "Unsupported distro: ${PRETTY_NAME:-unknown}" >&2
      echo "Please install manually: GTK4 dev libs, Wayland dev libs, rustup." >&2
      exit 1
    fi
    ;;
esac

if command -v rustup >/dev/null 2>&1; then
  rustup default stable
fi

echo "Dependency installation complete."
