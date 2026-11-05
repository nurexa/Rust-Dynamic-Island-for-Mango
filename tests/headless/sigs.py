#!/usr/bin/env python3
"""sigs.py FILE: append every org.freedesktop.Notifications signal to FILE, one per line,
   as "Member arg arg...". Signals only, so our own calls' replies never show up."""
import sys, dbus
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib
DBusGMainLoop(set_as_default=True)
out = open(sys.argv[1], 'a', buffering=1)
def on(*args, member=None):
    out.write(' '.join([member] + [str(a) for a in args]) + '\n')
dbus.SessionBus().add_signal_receiver(on, dbus_interface='org.freedesktop.Notifications', member_keyword='member')
GLib.MainLoop().run()
