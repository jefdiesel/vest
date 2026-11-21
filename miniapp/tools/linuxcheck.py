#!/usr/bin/env python3
"""Does the Linux build of a Basecamp module plugin load? Usage:
    linuxcheck.py <dev lib output> <extracted portable variant dir> [<qt lib dir> ...]

The Linux counterpart of loadcheck.py, for CI. GNU ld lets a shared object keep undefined
symbols, so a library the Rust staticlib needs but the link forgot only shows when Basecamp
dlopens the plugin. This catches it here instead:

  1. dev plugin (the nix output, RUNPATH into the store): dlopen with RTLD_NOW, which fails on
     the first unresolved symbol or missing library.
  2. portable plugin (what the .lgx carries): no /nix/store in any RUNPATH, every NEEDED library
     is either bundled beside it or one the host provides (glibc, the C++ runtime, Qt), and the
     plugin dlopens with RTLD_NOW against only its own folder plus the Qt libraries given.

Run it with a python linked against the same glibc as the build (the builder's nixpkgs).
"""
import ctypes, os, re, subprocess, sys

# What Basecamp's host process or the system provides; everything else must be bundled.
HOST = re.compile(r"^(ld-linux.*|libc|libm|libdl|libpthread|librt|libutil|libresolv|libgcc_s"
                  r"|libstdc\+\+|libQt6\w*)\.so(\.\d+)*$")


def elf_files(root):
    for d, _, names in os.walk(root):
        for n in names:
            p = os.path.join(d, n)
            if os.path.isfile(p) and not os.path.islink(p):
                with open(p, "rb") as f:
                    if f.read(4) == b"\x7fELF":
                        yield p


def dynamic(path):
    out = subprocess.run(["readelf", "-d", path], capture_output=True, text=True).stdout
    needed = re.findall(r"\(NEEDED\)\s+Shared library: \[(.+?)\]", out)
    runpath = re.findall(r"\((?:RUNPATH|RPATH)\)\s+Library (?:runpath|rpath): \[(.*?)\]", out)
    return needed, runpath


def plugin_in(root):
    plugins = [p for p in elf_files(root) if re.search(r"_plugin\.so$", p)]
    if len(plugins) != 1:
        sys.exit(f"expected one *_plugin.so under {root}, found {plugins}")
    return plugins[0]


def dlopen(path):
    try:
        ctypes.CDLL(path, mode=os.RTLD_NOW | os.RTLD_LOCAL)
        return None
    except OSError as error:
        return str(error)


def main():
    dev, portable, qt_dirs = sys.argv[1], sys.argv[2], sys.argv[3:]
    failed = False

    plugin = plugin_in(dev)
    needed, _ = dynamic(plugin)
    print(f"dev plugin {os.path.basename(plugin)} NEEDED: {', '.join(needed)}")
    error = dlopen(plugin)
    print("dev dlopen(RTLD_NOW):", error or "OK")
    failed |= error is not None

    plugin = plugin_in(portable)
    folder = os.path.dirname(plugin)
    bundled = {os.path.basename(p) for p in os.listdir(folder)}
    print(f"portable folder {folder}: {', '.join(sorted(bundled))}")
    for elf in elf_files(portable):
        needed, runpath = dynamic(elf)
        name = os.path.relpath(elf, portable)
        for rp in runpath:
            if "/nix/store" in rp:
                print(f"  STORE RUNPATH in {name}: {rp}")
                failed = True
        for lib in needed:
            if lib not in bundled and not HOST.match(lib):
                print(f"  NOT BUNDLED {lib} (needed by {name})")
                failed = True
    # Only the bundle and Qt: the store paths the dev plugin would find are out of reach.
    os.environ["LD_LIBRARY_PATH"] = ":".join([folder] + qt_dirs)
    probe = subprocess.run([sys.executable, "-c",
                            "import ctypes,os,sys; ctypes.CDLL(sys.argv[1], mode=os.RTLD_NOW)", plugin],
                           capture_output=True, text=True)
    error = (probe.stderr.strip().splitlines() or ["failed"])[-1] if probe.returncode else None
    print("portable dlopen(RTLD_NOW):", error or "OK")
    failed |= error is not None

    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
