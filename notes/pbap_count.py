"""Read-only PBAP probe: prints the size of the phonebook and combined call history.

Usage: python3 pbap_count.py AA:BB:CC:DD:EE:FF

Everything runs over ONE D-Bus connection, because obexd destroys a session as soon as
the connection that created it goes away.
"""
import sys

from gi.repository import Gio, GLib

address = sys.argv[1]
bus = Gio.bus_get_sync(Gio.BusType.SESSION)


def call(path, iface, method, sig=None, args=(), timeout=90000):
    return bus.call_sync("org.bluez.obex", path, iface, method,
        GLib.Variant(sig, args) if sig else None, None, 0, timeout, None).unpack()


(s,) = call("/org/bluez/obex", "org.bluez.obex.Client1", "CreateSession", "(sa{sv})",
            (address, {"Target": GLib.Variant("s", "PBAP")}))
pa = "org.bluez.obex.PhonebookAccess1"
try:
    for book, label in [("pb", "contacts"), ("cch", "call history (combined)")]:
        call(s, pa, "Select", "(ss)", ("int", book))
        print(f"{label}: {call(s, pa, 'GetSize')[0]}")
finally:
    call("/org/bluez/obex", "org.bluez.obex.Client1", "RemoveSession", "(o)", (s,))
