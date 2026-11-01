# Stand-in for Waybar's watcher: holds the name until killed.
import dbus, dbus.service, dbus.mainloop.glib, sys
from gi.repository import GLib
dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
n = dbus.service.BusName("org.kde.StatusNotifierWatcher", dbus.SessionBus())
GLib.MainLoop().run()
