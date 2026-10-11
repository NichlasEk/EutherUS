#!/usr/bin/env python3
"""Exercise the actual preview window with X11 input; never sends shell commands.
Run while regis-preview --state-file artifacts/regis/live-state.json is open.
Requires libX11 and libXtst; only injects input while that exact window has focus.
"""
import atexit
import subprocess
import sys
import ctypes as C
import json
from pathlib import Path
import time

ROOT=Path(__file__).resolve().parents[1]
SPAWN='--spawn' in sys.argv
STATE=ROOT/('artifacts/regis/native-state.json' if SPAWN else 'artifacts/regis/live-state.json')
if SPAWN:
    STATE.unlink(missing_ok=True)
    process=subprocess.Popen([str(ROOT/'target/release/regis-preview'),'--state-file',str(STATE)],cwd=ROOT)
    def shutdown():
        if process.poll() is None:
            process.terminate()
        process.wait(timeout=5)
    atexit.register(shutdown)

x=C.CDLL('libX11.so.6'); xt=C.CDLL('libXtst.so.6')
P=C.c_void_p; U=C.c_ulong; I=C.c_int

def define(lib,name,restype,args):
    f=getattr(lib,name);f.restype=restype;f.argtypes=args;return f

def wait(predicate,timeout=5):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            value=json.loads(STATE.read_text())
            if predicate(value):return value
        except (FileNotFoundError,json.JSONDecodeError):pass
        time.sleep(.05)
    raise AssertionError('native UI state did not reach expected result')

open_display=define(x,'XOpenDisplay',P,[C.c_char_p]);d=open_display(None)
if not d:raise SystemExit('No X11 display')
root=define(x,'XDefaultRootWindow',U,[P])(d)
query=define(x,'XQueryTree',I,[P,U,C.POINTER(U),C.POINTER(U),C.POINTER(C.POINTER(U)),C.POINTER(C.c_uint)])
fetch=define(x,'XFetchName',I,[P,U,C.POINTER(C.c_char_p)])
free=define(x,'XFree',I,[P]);flush=define(x,'XFlush',I,[P])
focus=define(x,'XSetInputFocus',I,[P,U,I,U]);getfocus=define(x,'XGetInputFocus',I,[P,C.POINTER(U),C.POINTER(I)])
keysym=define(x,'XStringToKeysym',U,[C.c_char_p]);keycode=define(x,'XKeysymToKeycode',C.c_ubyte,[P,U])
key=define(xt,'XTestFakeKeyEvent',I,[P,C.c_uint,I,U])
warp=define(x,'XWarpPointer',I,[P,U,U,I,I,C.c_uint,C.c_uint,I,I]);button=define(xt,'XTestFakeButtonEvent',I,[P,C.c_uint,I,U])
geom=define(x,'XGetGeometry',I,[P,U,C.POINTER(U),C.POINTER(I),C.POINTER(I),C.POINTER(C.c_uint),C.POINTER(C.c_uint),C.POINTER(C.c_uint),C.POINTER(C.c_uint)])

def find(w,depth=0):
    name=C.c_char_p(); title=''
    if fetch(d,w,C.byref(name)) and name.value:
        title=name.value.decode(errors='replace');free(name)
    if title=='System Regis — EutherUS / SIMULERAD PROTOTYP':return w
    if depth>4:return None
    r=U();p=U();children=C.POINTER(U)();count=C.c_uint()
    if query(d,w,C.byref(r),C.byref(p),C.byref(children),C.byref(count)):
        ids=[children[i] for i in range(count.value)]
        if children:free(children)
        for child in ids:
            found=find(child,depth+1)
            if found:return found
    return None

w=None
for _ in range(100):
    w=find(root)
    if w:break
    time.sleep(.1)
if not w:raise SystemExit('Preview window not found')
focus(d,w,1,0);flush(d);time.sleep(.1)

def guard():
    current=U();revert=I();getfocus(d,C.byref(current),C.byref(revert))
    if current.value!=w:raise RuntimeError('Focus changed; refusing to inject input')

def press(name):
    guard();code=keycode(d,keysym(name.encode()))
    if not code:raise ValueError(name)
    key(d,code,1,0);key(d,code,0,0);flush(d);time.sleep(.04)

def command(text):
    press('Escape')
    for ch in text:press('space' if ch==' ' else ch)
    press('Return')

def click(px,py):
    guard();r=U();a=I();b=I();ww=C.c_uint();hh=C.c_uint();border=C.c_uint();depth=C.c_uint()
    geom(d,w,C.byref(r),C.byref(a),C.byref(b),C.byref(ww),C.byref(hh),C.byref(border),C.byref(depth))
    scale=min(ww.value/1440,hh.value/900)
    px=int((ww.value-1440*scale)/2+px*scale);py=int((hh.value-900*scale)/2+py*scale)
    warp(d,0,w,0,0,0,0,px,py);flush(d);time.sleep(.08)
    button(d,1,1,0);flush(d);time.sleep(.08);button(d,1,0,0);flush(d)

initial=wait(lambda s:s['simulated'])
click(400,418);wait(lambda s:s['selected']==4)
base=initial['memory_mib']
command('start aurora 16')
created=wait(lambda s:any(n['name']=='aurora' for n in s['nodes']))
assert created['memory_mib']==base+16
child=created['selected']
command('alloc 8');wait(lambda s:s['memory_mib']==base+24)
if SPAWN:
    define(x,'XResizeWindow',I,[P,U,C.c_uint,C.c_uint])(d,w,830,1002);flush(d);time.sleep(.3)
click(1100,550);wait(lambda s:next(n for n in s['nodes'] if n['id']==child)['paused'])
click(1100,550);wait(lambda s:not next(n for n in s['nodes'] if n['id']==child)['paused'])
if SPAWN:
    subprocess.run(['magick','import','-window',str(w),str(ROOT/'artifacts/regis/window-tall.png')],check=True)
    define(x,'XResizeWindow',I,[P,U,C.c_uint,C.c_uint])(d,w,1800,700);flush(d);time.sleep(.3)
    click(400,418);wait(lambda s:s['selected']==4)
    click(400,468);wait(lambda s:s['selected']==child)
press('F2');wait(lambda s:not s['inspector'])
press('F2');wait(lambda s:s['inspector'])
command('stop');final=wait(lambda s:s['memory_mib']==base and s['selected']==4)
assert all(n['id']!=child for n in final['nodes'])
press('F10');time.sleep(.3)
report={'passed':True,'simulated':True,'window':w,'initial_mib':base,'final_mib':final['memory_mib'],
        'isolated_display':SPAWN,'checks':['mouse selection','typed start','typed allocation','mouse pause/resume','inspector keyboard toggle','typed stop','exact memory release','F10 screenshot']+(['tall/wide window pointer mapping'] if SPAWN else [])}
(ROOT/'artifacts/regis/native-test.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
define(x,'XCloseDisplay',I,[P])(d)
