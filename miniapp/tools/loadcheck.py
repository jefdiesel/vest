#!/usr/bin/env python3
"""Does a Basecamp module plugin load? Usage: loadcheck.py <plugin.dylib> <basecamp Frameworks dir>

Loads what Basecamp's host provides (its protocol/core libraries and Qt) globally, the plugin's
own bundled libraries, then lists EVERY undefined symbol of the plugin that nothing loaded
provides (all at once, not dyld's first one), and finally dlopens the plugin itself.
"""
import ctypes, glob, os, subprocess, sys

plugin, fw = sys.argv[1], sys.argv[2]
G = ctypes.RTLD_GLOBAL
loaded = []
for path in [os.path.join(fw, n) for n in ("liblogos_protocol.dylib", "liblogos_core.dylib", "liblogos_qt_host.dylib")] + \
        [f"{fw}/{q}.framework/Versions/A/{q}" for q in ("QtCore", "QtNetwork", "QtRemoteObjects")] + \
        sorted(p for p in glob.glob(os.path.join(os.path.dirname(plugin), "*.dylib")) if p != plugin):
    try:
        ctypes.CDLL(path, mode=G)
        loaded.append(os.path.basename(path))
    except OSError as error:
        print("could not preload", path, error)
# Everything the plugin links (otool -L): absolute paths as they are, @rpath ones from the host's
# Frameworks folder (as Basecamp's own rpath finds them; DYLD_* variables don't reach a SIP python).
for line in subprocess.run(["otool", "-L", plugin], capture_output=True, text=True).stdout.splitlines()[1:]:
    dep = line.strip().split(" (")[0]
    if dep.startswith("@loader_path"):
        continue
    path = dep.replace("@rpath", fw) if dep.startswith("@rpath") else dep
    try:
        ctypes.CDLL(path, mode=G)
        loaded.append(os.path.basename(path))
    except OSError as error:
        print("could not preload", dep, error)
print("preloaded:", ", ".join(loaded))

libc = ctypes.CDLL(None)
libc.dlsym.restype = ctypes.c_void_p
libc.dlsym.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
RTLD_DEFAULT = ctypes.c_void_p(-2)
undefined = subprocess.run(["nm", "-u", plugin], capture_output=True, text=True).stdout.split()
missing = [s for s in undefined if s.startswith("_") and not libc.dlsym(RTLD_DEFAULT, s[1:].encode())]
print(f"undefined symbols: {len(undefined)}, unresolved: {len(missing)}")
for s in missing:
    print("  MISSING", s)
try:
    ctypes.CDLL(plugin, mode=G)
    print("dlopen: OK")
except OSError as error:
    print("dlopen: FAILED", error)
    sys.exit(1)
sys.exit(1 if missing else 0)
