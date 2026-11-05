#!/usr/bin/env python3
"""A laptop battery for the test bus: UPower's display device, read from a JSON
file. SIGUSR1 re-reads the file and sends PropertiesChanged, as a plug-in would."""
import json, signal, sys
import dbus, dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

STATE = sys.argv[1]
DEV = "org.freedesktop.UPower.Device"

def load():
    d = json.load(open(STATE))
    return {"IsPresent": dbus.Boolean(True), "Type": dbus.UInt32(2), "State": dbus.UInt32(d["state"]),
            "Percentage": dbus.Double(d["pct"]), "TimeToEmpty": dbus.Int64(d.get("empty", 0)),
            "TimeToFull": dbus.Int64(d.get("full", 0))}

class Display(dbus.service.Object):
    @dbus.service.method(dbus.PROPERTIES_IFACE, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        return load()

    @dbus.service.method(dbus.PROPERTIES_IFACE, in_signature="ss", out_signature="v")
    def Get(self, iface, prop):
        return load()[prop]

    @dbus.service.signal(dbus.PROPERTIES_IFACE, signature="sa{sv}as")
    def PropertiesChanged(self, iface, changed, invalidated):
        pass

DBusGMainLoop(set_as_default=True)
bus = dbus.bus.BusConnection(sys.argv[2])
name = dbus.service.BusName("org.freedesktop.UPower", bus)
dev = Display(bus, "/org/freedesktop/UPower/devices/DisplayDevice")
GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, signal.SIGUSR1, lambda: dev.PropertiesChanged(DEV, load(), []) or True)
GLib.MainLoop().run()
