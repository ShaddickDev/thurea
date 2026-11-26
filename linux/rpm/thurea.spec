# Thurea for Fedora. Built by release/build.sh from a staged tree (linux/stage.sh … rpm):
#   rpmbuild -bb --define "_topdir …" --define "stage …" --define "ver …" thurea.spec
Name:           thurea
Version:        %{ver}
Release:        1
Summary:        Family protection: harmful-site and ad blocker, managed by the system
License:        MIT
URL:            https://github.com/ShaddickDev/thurea
Requires:       systemd, e2fsprogs, python3, nftables, openssl, curl
Recommends:     NetworkManager, android-tools, ffmpeg-free
AutoReqProv:    no
%global debug_package %{nil}
%global __strip /bin/true

%description
Thurea: hosts block, family DNS, browser policies with the Thurea extension force-installed,
DNS lockdown, household mesh with parents' controls, self-updating from signed GitHub releases.

%install
cp -a %{stage}/. %{buildroot}/

%files -f %{_topdir}/files.list

%post
systemctl daemon-reload || :
systemctl enable --now thurea.timer thurea.path thurea-extensions.service thurea-update.timer thurea-mesh.service >/dev/null 2>&1 || :
if [ "$1" -gt 1 ]; then systemctl restart thurea-extensions.service >/dev/null 2>&1 || :; systemctl try-restart thurea-mesh.service >/dev/null 2>&1 || :; fi
/usr/lib/thurea/thurea-apply || :
/usr/lib/thurea/thurea-omarchy install-all >/dev/null 2>&1 || :

%preun
# Thurea is part of this system: removing it is refused (upgrades are fine).
if [ "$1" -eq 0 ]; then
  echo "Thurea is part of this system and cannot be removed." >&2
  exit 1
fi
