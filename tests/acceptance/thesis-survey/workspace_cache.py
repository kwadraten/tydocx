"""Process-local Windows Known Folder override for the fixed survey EXE.

The embedded typst-kit 0.15.1 resolver ignores TYPST_PACKAGE_CACHE_PATH on
Windows. The review EXE has one import call to SHGetKnownFolderPath. Under the
standard Windows debug API we substitute only the returned Local/Roaming
AppData path in that child, before the path is read by the package resolver.
No persisted binary, registry, user profile, or system DLL change is made.
"""

import ctypes as C
from ctypes import wintypes as W
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import uuid

K = C.WinDLL("kernel32", use_last_error=True)
SIZE = C.c_size_t
U64 = C.c_uint64
GUIDS = {uuid.UUID(x).bytes_le: label for x, label in {
    "F1B32785-6FBA-4FCF-9D55-7B8E7F157091": "LocalAppData",
    "3EB685DB-65F9-4CF6-A03A-E3EF65729F3D": "RoamingAppData",
}.items()}


def api(name, restype, *args):
    f = getattr(K, name)
    f.restype, f.argtypes = restype, args
    return f


def must(value):
    if not value:
        raise C.WinError(C.get_last_error())
    return value


def known_folder_calls(exe):
    pe = exe.read_bytes()
    read = lambda fmt, off: struct.unpack_from("<" + fmt, pe, off)
    nt = read("I", 60)[0]
    arch, sec_count, _, _, _, opt_len, _ = read("HHIIIHH", nt + 4)
    opt = nt + 24
    if pe[nt:nt + 4] != b"PE\0\0" or arch != 0x8664 or read("H", opt)[0] != 0x20B:
        raise ValueError("this launcher only supports the fixed x86-64 PE")
    sec = opt + opt_len
    sections = [(pe[i:i+8].rstrip(b"\0"), *read("IIII", i + 8))
                for i in range(sec, sec + 40 * sec_count, 40)]

    def off(rva):
        for _, _, va, size, raw in sections:
            if va <= rva < va + size:
                return raw + rva - va
        raise ValueError(f"RVA not backed by a PE section: {rva:x}")

    def string(rva):
        begin = off(rva)
        return pe[begin:pe.index(b"\0", begin)]

    desc = off(read("I", opt + 120)[0])
    target = None
    while any(row := read("IIIII", desc)):
        original, _, _, dll, first = row
        if string(dll).lower() == b"shell32.dll":
            thunk = off(original or first)
            for index in range(300):
                rva = read("Q", thunk + index * 8)[0]
                if not rva:
                    break
                if rva < 1 << 63 and string(rva + 2) == b"SHGetKnownFolderPath":
                    target = first + index * 8
        desc += 20
    if target is None:
        raise ValueError("no SHGetKnownFolderPath import")
    matches = []
    for name, _, va, size, raw in sections:
        if name == b".text":
            for match in re.finditer(b"\xff\x15", pe[raw:raw + size]):
                i = match.start()
                if i + 6 <= size and va + i + 6 + read("i", raw + i + 2)[0] == target:
                    matches.append(va + i)
    if len(matches) != 1:
        raise ValueError(f"expected exactly one callsite, found {matches}")
    return matches[0]


class Startup(C.Structure):
    _fields_ = [
        ("cb", W.DWORD), ("reserved", W.LPWSTR), ("desktop", W.LPWSTR), ("title", W.LPWSTR),
        *[(n, W.DWORD) for n in ("x", "y", "xs", "ys", "xc", "yc", "fill", "flags")],
        ("show", W.WORD), ("unused", W.WORD), ("reserved2", C.c_void_p),
        ("stdin", W.HANDLE), ("stdout", W.HANDLE), ("stderr", W.HANDLE),
    ]


class ProcessInfo(C.Structure):
    _fields_ = [("process", W.HANDLE), ("thread", W.HANDLE), ("pid", W.DWORD), ("tid", W.DWORD)]


create = api("CreateProcessW", W.BOOL, W.LPCWSTR, W.LPWSTR, C.c_void_p, C.c_void_p, W.BOOL,
             W.DWORD, C.c_void_p, W.LPCWSTR, C.POINTER(Startup), C.POINTER(ProcessInfo))
wait = api("WaitForDebugEvent", W.BOOL, C.c_void_p, W.DWORD)
cont = api("ContinueDebugEvent", W.BOOL, W.DWORD, W.DWORD, W.DWORD)
openthread = api("OpenThread", W.HANDLE, W.DWORD, W.BOOL, W.DWORD)
close = api("CloseHandle", W.BOOL, W.HANDLE)
terminate = api("TerminateProcess", W.BOOL, W.HANDLE, W.UINT)
getcontext = api("GetThreadContext", W.BOOL, W.HANDLE, C.c_void_p)
setcontext = api("SetThreadContext", W.BOOL, W.HANDLE, C.c_void_p)
readprocess = api("ReadProcessMemory", W.BOOL, W.HANDLE, C.c_void_p, C.c_void_p, SIZE, C.POINTER(SIZE))
writeprocess = api("WriteProcessMemory", W.BOOL, W.HANDLE, C.c_void_p, C.c_void_p, SIZE, C.POINTER(SIZE))
flush = api("FlushInstructionCache", W.BOOL, W.HANDLE, C.c_void_p, SIZE)
std = api("GetStdHandle", W.HANDLE, W.DWORD)


def run(exe, args, base_dir):
    rva = known_folder_calls(exe)
    if C.sizeof(C.c_void_p) != 8:
        raise ValueError("requires x64 Python")
    if not (base_dir / "typst/packages").samefile(base_dir.parent / "package-cache"):
        raise ValueError("survey-local package junction does not target the workspace cache")
    startup, proc = Startup(), ProcessInfo()
    startup.cb = C.sizeof(startup)
    startup.flags = 0x100  # STARTF_USESTDHANDLES
    startup.stdin, startup.stdout, startup.stderr = [std(n) for n in (-10, -11, -12)]
    command = C.create_unicode_buffer(subprocess.list2cmdline([str(exe), *args]))
    must(create(str(exe), command, None, None, True, 0x2, None, str(base_dir),
                C.byref(startup), C.byref(proc)))  # DEBUG_ONLY_THIS_PROCESS
    event = C.create_string_buffer(192)
    addr, call_byte, ret_byte = None, b"", b""
    pending, stepped, seen = {}, set(), []
    initial_break = True

    def read(address, length):
        buf = C.create_string_buffer(length)
        must(readprocess(proc.process, address, buf, length, None))
        return buf.raw

    def write(address, data, code=False):
        must(writeprocess(proc.process, address, data, len(data), None))
        if code:
            must(flush(proc.process, address, len(data)))

    try:
        while True:
            must(wait(event, 0xFFFFFFFF))
            data = event.raw
            kind, pid, tid = struct.unpack_from("<III", data)
            payload = data[16:]
            disposition = 0x00010002  # DBG_CONTINUE
            if kind == 3:  # CREATE_PROCESS_DEBUG_EVENT: module base has already been mapped
                handle, _, _, imagebase = struct.unpack_from("<QQQQ", payload)
                if handle:
                    close(handle)
                addr = imagebase + rva
                if read(addr, 2) != b"\xff\x15":
                    raise ValueError("review import call bytes changed")
                call_byte, ret_byte = read(addr, 1), read(addr + 6, 1)
                write(addr, b"\xcc", True)
            elif kind == 6:  # LOAD_DLL_DEBUG_EVENT
                if handle := struct.unpack_from("<Q", payload)[0]:
                    close(handle)
            elif kind == 5:  # EXIT_PROCESS_DEBUG_EVENT
                rc = struct.unpack_from("<I", payload)[0]
                must(cont(pid, tid, disposition))
                print(f"workspace cache redirects: {', '.join(seen)}", file=sys.stderr, flush=True)
                if not seen and rc == 0:
                    raise ValueError("no Known Folder path redirected; not a valid survey run")
                return rc
            elif kind == 1:
                exception = struct.unpack_from("<I", payload)[0]
                ip = struct.unpack_from("<Q", payload, 16)[0]
                manage = ((exception == 0x80000003 and ip in (addr, addr + 6))
                          or (exception == 0x80000004 and tid in stepped))
                if manage:
                    thread = must(openthread(0x18, False, tid))  # GET/SET_CONTEXT
                    ctx = C.create_string_buffer(1250)
                    ctxptr = (C.addressof(ctx) + 15) & ~15
                    reg = lambda offset: U64.from_address(ctxptr + offset)
                    flags = W.DWORD.from_address(ctxptr + 68)
                    W.DWORD.from_address(ctxptr + 48).value = 0x100003  # AMD64 + CONTROL + INTEGER
                    try:
                        must(getcontext(thread, ctxptr))
                        if exception == 0x80000004:
                            stepped.remove(tid)
                            write(addr, b"\xcc", True)
                            flags.value &= ~0x100
                        elif ip == addr:
                            guid = read(reg(128).value, 16)
                            if label := GUIDS.get(guid):
                                if pending:
                                    raise ValueError("concurrent Known Folder return; abort instead of mixing")
                                pending[tid] = (reg(192).value, label)
                                write(addr + 6, b"\xcc", True)
                            write(addr, call_byte, True)
                            reg(248).value = addr
                            flags.value |= 0x100
                            stepped.add(tid)
                        else:
                            pointer, label = pending.pop(tid)
                            if reg(120).value & 0xffffffff != 0:
                                raise ValueError(f"system Known Folder call failed: {label}")
                            original = struct.unpack("<Q", read(pointer, 8))[0]
                            # Reuse the OS-allocated buffer: '.' plus UTF-16 NUL fits
                            # the existing absolute AppData string; deallocation unchanged.
                            if read(original, 4) in (b"\0" * 4,):
                                raise ValueError("empty Known Folder response")
                            write(original, b".\0\0\0")
                            seen.append(label)
                            write(addr + 6, ret_byte, True)
                            reg(248).value = addr + 6
                        must(setcontext(thread, ctxptr))
                    finally:
                        close(thread)
                elif exception == 0x80000003 and initial_break:
                    initial_break = False
                else:
                    disposition = 0x80010001  # DBG_EXCEPTION_NOT_HANDLED
            must(cont(pid, tid, disposition))
    except BaseException:
        terminate(proc.process, 1)
        raise
    finally:
        close(proc.thread)
        close(proc.process)


if __name__ == "__main__":
    exe = Path(sys.argv[1]).resolve()
    if exe.name != "tydocx-review.exe":
        raise SystemExit("This process-local adapter is restricted to the survey review executable.")
    sys.exit(run(exe, sys.argv[2:], Path(__file__).resolve().parent))
