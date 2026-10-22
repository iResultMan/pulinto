# Built from the release archive (see packaging/make-rpm.sh): the binaries are
# compiled beforehand, so rpmbuild needs no Rust toolchain.
%global debug_package %{nil}
%global __strip /bin/true

Name:           pulinto
Version:        %{pkg_version}
Release:        1
Summary:        Automatic keyboard layout switcher
License:        (MIT OR Apache-2.0) AND CC-BY-4.0
URL:            https://github.com/iResultMan/pulinto
Source0:        %{pkg_source}

Requires:       dbus-libs
Requires:       systemd-libs
Requires:       python3-gobject
Requires:       gtk4
Requires:       libadwaita
Recommends:     gnome-shell-extension-appindicator
Requires(post): systemd
Requires(preun): systemd

%description
A word typed in the wrong layout (ghbdtn instead of привет) is erased and
retyped correctly. Works on X11 and on GNOME with Wayland; Russian and
English. Pre-release.

%prep
%setup -q -n pulinto-%{version}-linux-%{_arch}

%build

%install
install -d %{buildroot}%{_bindir} %{buildroot}%{_datadir}/pulinto \
    %{buildroot}%{_prefix}/lib/udev/rules.d %{buildroot}%{_datadir}/applications \
    %{buildroot}%{_datadir}/icons/hicolor/scalable/apps \
    %{buildroot}%{_datadir}/gnome-shell/extensions %{buildroot}%{_prefix}/lib/systemd/user \
    %{buildroot}%{_docdir}/pulinto
install -m755 bin/pulintod bin/pulinto %{buildroot}%{_bindir}/
install -m755 gui/pulinto-settings %{buildroot}%{_bindir}/
cp -a data/dict data/languages data/locale %{buildroot}%{_datadir}/pulinto/
install -m644 data/70-pulinto.rules %{buildroot}%{_prefix}/lib/udev/rules.d/
install -m644 data/ru.pulinto.Settings.desktop %{buildroot}%{_datadir}/applications/
install -m644 data/icons/ru.pulinto.Settings.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/
cp -a gnome-extension/pulinto@pulinto.ru %{buildroot}%{_datadir}/gnome-shell/extensions/
sed 's#^ExecStart=.*#ExecStart=%{_bindir}/pulintod#' data/pulintod.service > %{buildroot}%{_prefix}/lib/systemd/user/pulintod.service
install -m644 README.md README_EN.md NOTICE LICENSE-MIT LICENSE-APACHE LICENSE-CC-BY-4.0 %{buildroot}%{_docdir}/pulinto/

%post
udevadm control --reload >/dev/null 2>&1 || :
udevadm trigger --subsystem-match=input --subsystem-match=misc >/dev/null 2>&1 || :
systemctl --global enable pulintod.service >/dev/null 2>&1 || :
gtk-update-icon-cache -q -t -f %{_datadir}/icons/hicolor >/dev/null 2>&1 || :
# Start (or restart after an upgrade) the daemon for everyone who is logged in on a graphical
# session; the others get it at their next login. No second step for the user.
if command -v loginctl >/dev/null 2>&1 && command -v runuser >/dev/null 2>&1; then
    loginctl list-sessions --no-legend 2>/dev/null | while read -r id uid user _rest; do
        type="$(loginctl show-session "$id" -p Type --value 2>/dev/null || true)"
        case "$type" in x11|wayland) ;; *) continue ;; esac
        [ -S "/run/user/$uid/bus" ] || continue
        runuser -u "$user" -- env XDG_RUNTIME_DIR="/run/user/$uid" \
            DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$uid/bus" \
            sh -c 'systemctl --user daemon-reload && systemctl --user restart pulintod.service' >/dev/null 2>&1 || true
    done
fi
case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
    ru*) echo "PuLinTo установлен и запущен." ;;
    *) echo "PuLinTo is installed and running." ;;
esac

%preun
if [ $1 -eq 0 ]; then
    systemctl --global disable pulintod.service >/dev/null 2>&1 || :
fi

%postun
# The udev rule left with the package; drop the access to the keyboards now, not at the next boot.
if [ $1 -eq 0 ]; then
    udevadm control --reload >/dev/null 2>&1 || :
    udevadm trigger --subsystem-match=input --subsystem-match=misc >/dev/null 2>&1 || :
fi

%files
%{_bindir}/pulintod
%{_bindir}/pulinto
%{_bindir}/pulinto-settings
%{_datadir}/pulinto
%{_prefix}/lib/udev/rules.d/70-pulinto.rules
%{_datadir}/applications/ru.pulinto.Settings.desktop
%{_datadir}/icons/hicolor/scalable/apps/ru.pulinto.Settings.svg
%{_datadir}/gnome-shell/extensions/pulinto@pulinto.ru
%{_prefix}/lib/systemd/user/pulintod.service
%doc %{_docdir}/pulinto/README.md
%doc %{_docdir}/pulinto/README_EN.md
%license %{_docdir}/pulinto/NOTICE
%license %{_docdir}/pulinto/LICENSE-MIT
%license %{_docdir}/pulinto/LICENSE-APACHE
%license %{_docdir}/pulinto/LICENSE-CC-BY-4.0

%changelog
* Tue Oct 20 2026 Yury ResultMan <i.result.man@gmail.com> - 1.0.0-1
- First pre-release package.
