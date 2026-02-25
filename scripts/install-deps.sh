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
    systemd \
    gtk4 \
    gtk4-layer-shell \
    wayland \
    wayland-protocols \
    rustup
}

install_debian() {
  ${SUDO} apt update

  if ! apt-cache show libgtk4-layer-shell-dev >/dev/null 2>&1; then
    echo "Missing package: libgtk4-layer-shell-dev" >&2
    echo "This distro release does not provide GTK4 layer-shell development headers." >&2
    echo "Use Ubuntu 24.04+ (or Debian testing/unstable), Fedora, or Arch for now." >&2
    exit 1
  fi

  ${SUDO} apt install -y \
    build-essential \
    pkg-config \
    libudev-dev \
    libgtk-4-dev \
    libgtk4-layer-shell-dev \
    libwayland-dev \
    wayland-protocols \
    curl

  if ! command -v rustup >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  fi
}

install_fedora() {
  ${SUDO} dnf install -y \
    make \
    gcc \
    pkgconf-pkg-config \
    systemd-devel \
    gtk4-devel \
    gtk4-layer-shell-devel \
    wayland-devel \
    wayland-protocols-devel \
    rustup
}

case "${ID:-}" in
  arch|cachyos|endeavouros|manjaro)
    install_arch
    ;;
  ubuntu|debian|linuxmint|pop)
    install_debian
    ;;
  fedora|rhel|centos)
    install_fedora
    ;;
  *)
    if [[ "${ID_LIKE:-}" == *"arch"* ]]; then
      install_arch
    elif [[ "${ID_LIKE:-}" == *"fedora"* ]] || [[ "${ID_LIKE:-}" == *"rhel"* ]]; then
      install_fedora
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
