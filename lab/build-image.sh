#!/bin/bash
# Builds the lab VM image from files already on this computer — no downloads:
# the running Omarchy kernel, its virtio network driver, bash/coreutils/iproute2 from the
# host, and the freshly built `thurea` binary. Output: lab/out/initramfs.img + vmlinuz.
set -euo pipefail
L=$(cd "$(dirname "$0")" && pwd); ROOT=$(cd "$L/.." && pwd)
K=/usr/lib/modules/$(uname -r)
O=$L/out; R=$O/root
rm -rf "$R"; mkdir -p "$R"/{bin,lib64,usr/lib,etc,proc,sys,dev,tmp,run,var/lib,root,mod}
ln -s ../lib64 "$R/usr/lib64"; ln -s bin "$R/sbin"; ln -s ../bin "$R/usr/bin"; ln -s usr/lib "$R/lib"

THUREA=$ROOT/target/release/thurea
[ -x "$THUREA" ] || (cd "$ROOT" && PATH=$HOME/.cargo/bin:$PATH cargo build --release -p thurea-agent)
TOOLS="bash cat ls sleep mkdir cp rm mv grep sed head tail date cut tr wc sort uniq env id ip kmod uname seq tee mount"
for t in $TOOLS; do cp -L "$(command -v "$t")" "$R/bin/"; done
cp "$THUREA" "$R/bin/thurea"
ln -s kmod "$R/bin/insmod"
# Shared libraries of everything copied.
for b in "$R"/bin/*; do
  ldd "$b" 2>/dev/null | grep -oE '/[^ ]+\.so[^ ]*' || true
done | sort -u | while read -r lib; do
  case "$lib" in */ld-linux*) cp -L "$lib" "$R/lib64/";; *) cp -L "$lib" "$R/usr/lib/";; esac
done
# virtio-net driver (zstd modules unpacked, loaded in dependency order by init).
for m in kernel/net/core/failover kernel/drivers/net/net_failover kernel/drivers/net/virtio_net; do
  zstd -qdc "$K/$m.ko.zst" > "$R/mod/$(basename "$m").ko"
done
printf '127.0.0.1 localhost\n::1 localhost\n' > "$R/etc/hosts"
printf 'root:x:0:0:root:/root:/bin/bash\n' > "$R/etc/passwd"
cp "$L/init" "$R/init"; chmod +x "$R/init"
(cd "$R" && find . | cpio -o -H newc --quiet | zstd -q -19 -T0 > "$O/initramfs.img")
cp "$K/vmlinuz" "$O/vmlinuz"
echo "Image: $O/initramfs.img ($(du -h "$O/initramfs.img" | cut -f1)), kernel $(uname -r)"
