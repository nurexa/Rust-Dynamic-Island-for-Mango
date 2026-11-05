# A pretend BlueZ on whatever bus DBUS_SYSTEM_BUS_ADDRESS names: one adapter, a
# connected headset with a battery, a paired mouse, and a speaker that only shows
# up while scanning. Every call that would change something is logged to argv[1],
# so a test can see what the island asked for without real hardware involved.
#   python3 fakebt.py LOG
# test.Control on / lets a test move things: SetBattery(path, pct), Adapter(present).
import dbus, dbus.service, dbus.mainloop.glib, os, sys
from gi.repository import GLib

dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
bus = dbus.bus.BusConnection(os.environ["DBUS_SYSTEM_BUS_ADDRESS"])
LOG = open(sys.argv[1], "a", buffering=1)
PROPS, OM = "org.freedesktop.DBus.Properties", "org.freedesktop.DBus.ObjectManager"
AD, DEV, BAT = "org.bluez.Adapter1", "org.bluez.Device1", "org.bluez.Battery1"
HCI = "/org/bluez/hci0"
objects = {}


class Obj(dbus.service.Object):
    def __init__(self, path, ifaces):
        self.path, self.ifaces = path, ifaces
        dbus.service.Object.__init__(self, bus, path)
        objects[path] = self
        root.InterfacesAdded(path, ifaces)

    def gone(self):
        del objects[self.path]
        root.InterfacesRemoved(self.path, list(self.ifaces))
        self.remove_from_connection()

    def change(self, iface, **kv):
        self.ifaces[iface].update(kv)
        self.PropertiesChanged(iface, kv, [])

    @dbus.service.method(PROPS, in_signature="ss", out_signature="v")
    def Get(self, iface, name):
        return self.ifaces[iface][name]

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        return self.ifaces.get(iface, {})

    @dbus.service.method(PROPS, in_signature="ssv")
    def Set(self, iface, name, value):
        LOG.write(f"set {self.path} {name} {bool(value)}\n")
        self.change(iface, **{name: value})
        if name == "Powered" and not value:
            for o in objects.values():
                if DEV in o.ifaces and o.ifaces[DEV]["Connected"]:
                    o.change(DEV, Connected=False)

    @dbus.service.signal(PROPS, signature="sa{sv}as")
    def PropertiesChanged(self, iface, changed, invalidated):
        pass


class Adapter(Obj):
    @dbus.service.method(AD)
    def StartDiscovery(self):
        LOG.write("discovery start\n")
        self.change(AD, Discovering=True)
        # A speaker in pairing mode turns up, and keeps reporting its signal strength,
        # as do the paired devices: chatter the island must not redraw for.
        GLib.timeout_add(800, lambda: device("dev_SPK", "Test Speaker", "audio-card", False, False))
        def chatter():
            if not self.ifaces[AD]["Discovering"]:
                return False
            for o in list(objects.values()):
                if DEV in o.ifaces:
                    o.change(DEV, RSSI=dbus.Int16(-40 - len(o.path) % 7))
            return True
        GLib.timeout_add(100, chatter)

    @dbus.service.method(AD)
    def StopDiscovery(self):
        LOG.write("discovery stop\n")
        self.change(AD, Discovering=False)


class Device(Obj):
    def _ok(self, what):
        LOG.write(f"{what} {self.path}\n")
        if "Broken" in self.ifaces[DEV]["Name"]:
            raise dbus.exceptions.DBusException("Page timeout", name="org.bluez.Error.Failed")

    @dbus.service.method(DEV)
    def Connect(self):
        self._ok("connect")
        self.change(DEV, Connected=True)

    @dbus.service.method(DEV)
    def Disconnect(self):
        self._ok("disconnect")
        self.change(DEV, Connected=False)

    @dbus.service.method(DEV)
    def Pair(self):
        self._ok("pair")
        self.change(DEV, Paired=True)


def device(name, alias, icon, paired, connected, battery=None):
    ifaces = {DEV: {"Name": alias, "Alias": alias, "Icon": icon, "Paired": paired, "Trusted": paired,
                    "Connected": connected, "Adapter": dbus.ObjectPath(HCI), "RSSI": dbus.Int16(-50)}}
    if battery is not None:
        ifaces[BAT] = {"Percentage": dbus.Byte(battery)}
    if f"{HCI}/{name}" not in objects:
        Device(f"{HCI}/{name}", ifaces)


def adapter():
    Adapter(HCI, {AD: {"Powered": True, "Discovering": False, "Alias": "fakebt"}})
    device("dev_HS", "Test Headset", "audio-headset", True, True, 80)
    device("dev_MS", "Test Mouse", "input-mouse", True, False)
    device("dev_BR", "Broken Pad", "input-gaming", True, False)


class Root(dbus.service.Object):
    @dbus.service.method(OM, out_signature="a{oa{sa{sv}}}")
    def GetManagedObjects(self):
        return {p: o.ifaces for p, o in objects.items()}

    @dbus.service.signal(OM, signature="oa{sa{sv}}")
    def InterfacesAdded(self, path, ifaces):
        pass

    @dbus.service.signal(OM, signature="oas")
    def InterfacesRemoved(self, path, ifaces):
        pass

    @dbus.service.method("test.Control", in_signature="oy")
    def SetBattery(self, path, pct):
        objects[path].change(BAT, Percentage=dbus.Byte(pct))

    @dbus.service.method("test.Control", in_signature="b")
    def Adapter(self, present):
        if present and HCI not in objects:
            adapter()
        elif not present:
            for p in sorted(objects, reverse=True):
                objects[p].gone()


root = Root(bus, "/")
adapter()
name = dbus.service.BusName("org.bluez", bus)
GLib.MainLoop().run()
