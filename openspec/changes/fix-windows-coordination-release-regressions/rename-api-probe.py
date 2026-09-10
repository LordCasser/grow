"""Isolated Win32 rename experiment; no Grow process or user storage involved."""
import ctypes as c
from ctypes import wintypes as w
from pathlib import Path
import tempfile
import json

kernel = c.WinDLL('kernel32', use_last_error=True)
kernel.CreateFileW.argtypes = [w.LPCWSTR, w.DWORD, w.DWORD, c.c_void_p, w.DWORD, w.DWORD, w.HANDLE]
kernel.CreateFileW.restype = w.HANDLE
kernel.SetFileInformationByHandle.argtypes = [w.HANDLE, c.c_int, c.c_void_p, w.DWORD]
kernel.SetFileInformationByHandle.restype = w.BOOL
kernel.CloseHandle.argtypes = [w.HANDLE]

class RenameInfo(c.Structure):
    _fields_ = [('flags', w.DWORD), ('root', w.HANDLE), ('length', w.DWORD), ('name', c.c_uint16 * 1)]

with tempfile.TemporaryDirectory(prefix='grow-rename-probe-') as temporary:
    root = Path(temporary)
    for long_path in (False, True):
        parent = root / ('long' if long_path else 'short')
        if long_path:
            parent = parent / ('profile' * 20) / ('storage' * 20)
        parent.mkdir(parents=True)
        for directory in (False, True):
            for kind in (3, 22):  # FileRenameInfo, FileRenameInfoEx
                for terminated in (False, True):
                    label = f'{directory}-{kind}-{terminated}'
                    source = parent / ('source-' + label)
                    target = parent / ('target-' + label)
                    if directory:
                        source.mkdir()
                    else:
                        source.write_bytes(b'source')
                    wide_target = str(target.resolve()).encode('utf-16-le')
                    # resolve() may omit the verbatim prefix in Python.
                    if not str(target.resolve()).startswith('\\\\?\\'):
                        wide_target = ('\\\\?\\' + str(target.resolve())).encode('utf-16-le')
                    length = RenameInfo.name.offset + len(wide_target)
                    # Nonterminated cases use an in-allocation poison tail to
                    # expose an API read past the counted name deterministically.
                    tail = b'\0\0' if terminated else 'POISON'.encode('utf-16-le') + b'\0\0'
                    buffer = c.create_string_buffer(length + len(tail))
                    info = RenameInfo.from_buffer(buffer)
                    info.flags, info.root, info.length = 0, None, len(wide_target)
                    c.memmove(c.addressof(buffer) + RenameInfo.name.offset, wide_target + tail, len(wide_target + tail))
                    source_path = '\\\\?\\' + str(source)
                    handle = kernel.CreateFileW(source_path, 0x10000, 3, None, 3, 0x02200000, None)
                    if handle == c.c_void_p(-1).value:
                        raise c.WinError(c.get_last_error())
                    try:
                        result = bool(kernel.SetFileInformationByHandle(handle, kind, buffer, length + (2 if terminated else 0)))
                        error = c.get_last_error() if not result else 0
                    finally:
                        kernel.CloseHandle(handle)
                    names = [p.name for p in parent.iterdir() if p.name.startswith('target-' + label)]
                    print(json.dumps(dict(long=long_path, directory=directory, kind=kind, terminated=terminated, ok=result, error=error, exact=target.exists(), names=names)), flush=True)
                    if terminated:
                        assert result and target.exists(), (error, names)
