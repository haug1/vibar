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
  fedora|rhel|centos)
    install_fedora
    ;;
  *)
    if [[ "${ID_LIKE:-}" == *"arch"* ]]; then
      install_arch
    elif [[ "${ID_LIKE:-}" == *"fedora"* ]] || [[ "${ID_LIKE:-}" == *"rhel"* ]]; then
      install_fedora
    else
      echo "Unsupported distro: ${PRETTY_NAME:-unknown}" >&2
      echo "Supported by this script: Arch-based and Fedora/RHEL-based distros." >&2
      exit 1
    fi
    ;;
esac

if command -v rustup >/dev/null 2>&1; then
  rustup default stable
fi

echo "Dependency installation complete."
