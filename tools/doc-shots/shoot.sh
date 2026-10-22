#!/bin/bash
# Screenshots of the settings window for the manual: Xvfb + private D-Bus + a profile with example rules,
# daemon in mode off. One language per call; one raw PNG per page in <scratch>/raw/.
#
#   tools/doc-shots/shoot.sh <scratch dir> <repository> ru_RU      (then en_US)
#   python3 -I tools/doc-shots/annotate.py <scratch dir>/raw docs/img/manual
#   python3 -I tools/doc-shots/tray_menu.py docs/img/manual
#
# The daemon only runs in the private session and never takes the real keyboards.
set -u
S=$1; REPO=$2; L=$3
T=$(mktemp -d -p "$S" ms.XXXXXX)
export HOME=$T/home XDG_CONFIG_HOME=$T/cfg XDG_DATA_HOME=$T/data XDG_CACHE_HOME=$T/cache XDG_STATE_HOME=$T/state
mkdir -p $HOME $XDG_CONFIG_HOME/pulinto $XDG_DATA_HOME/pulinto $XDG_CACHE_HOME $XDG_STATE_HOME $S/raw
printf 'mode = off\n' > $XDG_CONFIG_HOME/pulinto/config.conf
printf 'ещ\\\tещё\nспс\tспасибо\nнзч\tне за что\nмнгр\tмного\nприв\tпривет\n' > $XDG_DATA_HOME/pulinto/replacements.tsv
printf 'kubectl\nnginx\nsystemctl\n' > $XDG_DATA_HOME/pulinto/never.txt
printf 'rhby;\nghbrjk\n' > $XDG_DATA_HOME/pulinto/always.txt
printf 'Alacritty\n' > $XDG_DATA_HOME/pulinto/excluded-apps.txt
printf 'Code\n' > $XDG_DATA_HOME/pulinto/manual-apps.txt
chmod 600 $XDG_DATA_HOME/pulinto/*
unset WAYLAND_DISPLAY LANGUAGE LC_ALL LC_MESSAGES PULINTO_LANG PULINTO_SETTINGS_PAGE
export LANG=$L.UTF-8 XDG_SESSION_TYPE=x11 DISPLAY=:95 GDK_BACKEND=x11 XDG_CURRENT_DESKTOP=test GSETTINGS_BACKEND=memory ADW_DEBUG_COLOR_SCHEME=prefer-light
Xvfb :95 -screen 0 960x640x24 >$T/xvfb.log 2>&1 & XV=$!
sleep 1
dbus-run-session -- bash -c '
  "'$REPO'/target/release/pulintod" >'$T'/daemon.log 2>&1 & D=$!
  sleep 3
  for page in ${PAGES:-overview replacements never always apps keyboard hotkeys diagnostics help}; do
    PULINTO_SETTINGS_PAGE=$page python3 "'$REPO'/gui/pulinto-settings" >'$T'/gui-$page.log 2>&1 & G=$!
    sleep ${GUI_WAIT:-9}
    import -window root '$S'/raw/'$L'-$page.png
    if [ $page = replacements ]; then
      gdbus call --session --dest ru.pulinto.Settings --object-path /ru/pulinto/Settings/window/1 --method org.gtk.Actions.Activate add "[]" "{}" >/dev/null 2>&1
      sleep 2
      import -window root '$S'/raw/'$L'-add-dialog.png
    fi
    kill $G 2>/dev/null; sleep 1; kill -9 $G 2>/dev/null
  done
  kill $D 2>/dev/null; sleep 1; kill -9 $D 2>/dev/null
'
kill $XV 2>/dev/null
echo "done $L: $T"
