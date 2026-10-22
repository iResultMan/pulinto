// PuLinTo focus: which application has the keyboard, for the PuLinTo daemon.
//
// Under Wayland the daemon cannot ask for it: the X server is Xwayland and
// knows only X windows, and GNOME Shell's own introspection is closed to
// ordinary clients. Per-application rules — off in password managers, Alt-only
// in terminals — would silently stop applying to every Wayland window. This
// publishes the focused window's class (the app id for Wayland clients,
// WM_CLASS for X11 ones) on the session bus and announces every change.
// Nothing about the text typed is visible here or sent anywhere.
//
// While the Shell itself takes the keyboard — the lock screen, a password
// prompt (polkit, keyring), the overview search — the window underneath still
// counts as focused, yet the keys go to the Shell. Those moments are reported
// as `gnome-shell`, which the daemon leaves alone: a password typed to unlock
// the screen must never be "corrected". That is also why the extension stays
// enabled on the lock screen (`session-modes` in metadata.json).

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const BUS_NAME = 'ru.pulinto.Shell1';
const OBJECT_PATH = '/ru/pulinto/Shell1';
const INTERFACE = `
<node>
  <interface name="ru.pulinto.Shell1">
    <method name="FocusedApp">
      <arg type="s" direction="out" name="app"/>
    </method>
    <signal name="FocusChanged">
      <arg type="s" name="app"/>
    </signal>
  </interface>
</node>`;

export default class PulintoFocusExtension extends Extension {
    enable() {
        this._object = Gio.DBusExportedObject.wrapJSObject(INTERFACE, this);
        this._object.export(Gio.DBus.session, OBJECT_PATH);
        this._owner = Gio.bus_own_name(Gio.BusType.SESSION, BUS_NAME,
            Gio.BusNameOwnerFlags.NONE, null, null, null);
        this._focusId = global.display.connect('notify::focus-window',
            () => this._announce());
        // A modal dialog or the lock screen takes the stage's key focus.
        this._stageId = global.stage.connect('notify::key-focus',
            () => this._announce());
        this._modeId = Main.sessionMode.connect('updated', () => this._announce());
        this._last = null;
        this._lastWindow = null;
        this._announce();
    }

    disable() {
        global.display.disconnect(this._focusId);
        global.stage.disconnect(this._stageId);
        Main.sessionMode.disconnect(this._modeId);
        this._focusId = this._stageId = this._modeId = 0;
        Gio.bus_unown_name(this._owner);
        this._object.unexport();
        this._object = null;
    }

    FocusedApp() {
        if (Main.sessionMode.isLocked || Main.modalCount > 0)
            return 'gnome-shell';
        const window = global.display.focus_window;
        return window ? (window.get_wm_class() ?? '') : '';
    }

    _announce() {
        const app = this.FocusedApp();
        const window = global.display.focus_window;
        // Key focus moves inside an application all the time; news is a
        // change of the answer — or of the window behind it: the daemon drops
        // the word it is holding when the focus moves to another window, and
        // two windows of one application (two terminals) are two windows.
        if (app === this._last && window === this._lastWindow)
            return;
        this._last = app;
        this._lastWindow = window;
        this._object?.emit_signal('FocusChanged', new GLib.Variant('(s)', [app]));
    }
}
