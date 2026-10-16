#!/usr/bin/env bash
# Instala o Vortex Launcher a partir da última release do GitHub.
# curl -fsSL https://raw.githubusercontent.com/rafaelmarquesmatos/vortex-launcher/main/install.sh | bash
set -euo pipefail

repo="rafaelmarquesmatos/vortex-launcher"
api="https://api.github.com/repos/${repo}/releases/latest"
icon_url="https://raw.githubusercontent.com/${repo}/main/linux/src-tauri/icons/128x128.png"
data="${XDG_DATA_HOME:-$HOME/.local/share}/vortex-launcher"
bin_dir="$HOME/.local/bin"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"

pick_url() {
  python3 -c '
import json, sys
release = json.load(sys.stdin)
assets = release.get("assets") or []

def rank(name):
    lowered = name.lower()
    if "linux" not in lowered:
        return None
    if lowered.endswith(".appimage"):
        kind = 0
    elif lowered.endswith(".tar.gz"):
        kind = 1
    else:
        return None
    arch = 0 if ("x86_64" in lowered or "amd64" in lowered) else 1
    return (arch, kind, name)

ranked = []
for asset in assets:
    name = asset.get("name") or ""
    score = rank(name)
    if score is None:
        continue
    ranked.append((score, asset.get("browser_download_url") or ""))
if not ranked:
    sys.exit(2)
ranked.sort(key=lambda item: item[0])
url = ranked[0][1]
if not url:
    sys.exit(2)
print(url)
print(ranked[0][0][2], file=sys.stderr)
'
}

# shortcut: estes quatro caminhos se o ldconfig não listar a lib; alarga se a distro usar outro diretório.
# WEBKIT_LDCONFIG, quando definida, substitui `ldconfig -p` (só para --self-check).
has_webkit4_1() {
  local listing="" ld="" path
  if [ -n "${WEBKIT_LDCONFIG+x}" ]; then
    listing="$WEBKIT_LDCONFIG"
  else
    ld="$(command -v ldconfig 2>/dev/null || true)"
    if [ -z "$ld" ]; then
      if [ -x /usr/sbin/ldconfig ]; then
        ld=/usr/sbin/ldconfig
      elif [ -x /sbin/ldconfig ]; then
        ld=/sbin/ldconfig
      fi
    fi
    if [ -n "$ld" ]; then
      listing="$("$ld" -p 2>/dev/null || true)"
    fi
  fi
  if printf '%s\n' "$listing" | grep -q 'libwebkit2gtk-4\.1\.so'; then
    return 0
  fi
  if [ -n "${WEBKIT_LDCONFIG+x}" ]; then
    return 1
  fi
  for path in \
    /usr/lib64/libwebkit2gtk-4.1.so.0 \
    /usr/lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so.0 \
    /usr/lib/aarch64-linux-gnu/libwebkit2gtk-4.1.so.0 \
    /usr/lib/libwebkit2gtk-4.1.so.0
  do
    if [ -e "$path" ]; then
      return 0
    fi
  done
  return 1
}

require_webkit() {
  if has_webkit4_1; then
    return 0
  fi
  cat >&2 <<'EOF'
Falta o WebKitGTK 4.1. Instala o pacote da tua distro e corre o instalador outra vez:
  Debian/Ubuntu: libwebkit2gtk-4.1-0
  Fedora: webkit2gtk4.1
  Arch: webkit2gtk-4.1
EOF
  exit 1
}

if [ "${1:-}" = "--pick-url" ]; then
  pick_url
  exit 0
fi

if [ "${1:-}" = "--self-check" ]; then
  fail=0
  WEBKIT_LDCONFIG='libwebkit2gtk-4.1.so.0 (libc6,x86-64) => /usr/lib64/libwebkit2gtk-4.1.so.0'
  has_webkit4_1 || fail=1
  WEBKIT_LDCONFIG='libwebkit2gtk-4.0.so.37 (libc6,x86-64) => /usr/lib/libwebkit2gtk-4.0.so.37'
  has_webkit4_1 && fail=1
  WEBKIT_LDCONFIG='libwebkit2gtk-4.10.so.0 (libc6,x86-64) => /usr/lib/libwebkit2gtk-4.10.so.0'
  has_webkit4_1 && fail=1
  WEBKIT_LDCONFIG=''
  has_webkit4_1 && fail=1
  if [ "$fail" -ne 0 ]; then
    echo "self-check do WebKit falhou" >&2
    exit 1
  fi
  echo "self-check ok"
  exit 0
fi

if [ "$(uname -s)" != "Linux" ]; then
  echo "Este instalador só corre em Linux." >&2
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "Falta o python3 para ler a release." >&2
  exit 1
fi
require_webkit

echo "A procurar a última release…"
if ! json="$(curl -fsSL -H "Accept: application/vnd.github+json" -H "User-Agent: VortexLauncher" "$api")"; then
  echo "Ainda não há release em https://github.com/${repo}/releases" >&2
  exit 1
fi

if ! url="$(printf '%s' "$json" | pick_url)"; then
  echo "A release não tem um asset linux (.AppImage ou .tar.gz)." >&2
  exit 1
fi

name="${url##*/}"
mkdir -p "$data" "$bin_dir" "$apps"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "A baixar ${name}…"
curl -fL --retry 3 -o "$tmp/$name" "$url"

lower="$(printf '%s' "$name" | tr '[:upper:]' '[:lower:]')"
rm -f "$data/vortex-launcher.AppImage"
rm -rf "$data/pkg"
case "$lower" in
  *.appimage)
    install -m 755 "$tmp/$name" "$data/vortex-launcher.AppImage"
    bin_path="$data/vortex-launcher.AppImage"
    appimage=1
    ;;
  *.tar.gz)
    tar -xzf "$tmp/$name" -C "$tmp"
    found="$(find "$tmp" -type f \( -iname '*.appimage' -o -iname '*vortex*' \) ! -name "$name" | head -n 1)"
    if [ -z "$found" ]; then
      echo "O arquivo não tem o executável." >&2
      exit 1
    fi
    mkdir -p "$data/pkg"
    cp -a "$found" "$data/pkg/"
    bin_name="$(basename "$found")"
    chmod 755 "$data/pkg/$bin_name"
    bin_path="$data/pkg/$bin_name"
    found_lower="$(printf '%s' "$bin_name" | tr '[:upper:]' '[:lower:]')"
    if [[ "$found_lower" == *.appimage ]]; then
      appimage=1
    else
      appimage=0
    fi
    ;;
  *)
    echo "Asset inesperado: $name" >&2
    exit 1
    ;;
esac
if [ "$appimage" = 1 ]; then
  cat > "$bin_dir/vortex-launcher" <<EOF
#!/bin/sh
exec "$bin_path" --appimage-extract-and-run "\$@"
EOF
else
  cat > "$bin_dir/vortex-launcher" <<EOF
#!/bin/sh
exec "$bin_path" "\$@"
EOF
fi
chmod 755 "$bin_dir/vortex-launcher"

if curl -fsSL -o "$data/icon.png" "$icon_url"; then
  icon="$data/icon.png"
else
  icon="vortex-launcher"
fi

cat > "$apps/vortex-launcher.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Vortex Launcher
Comment=Launcher Linux para o Vortex
Exec=$bin_dir/vortex-launcher
Icon=$icon
Terminal=false
Categories=Game;
EOF

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$apps" >/dev/null 2>&1 || true
fi

echo "Instalado. Abre pelo menu ou com: $bin_dir/vortex-launcher"
case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "A pasta $bin_dir não está no PATH. O menu de aplicações abre na mesma." ;;
esac
