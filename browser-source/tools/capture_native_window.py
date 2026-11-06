"""Capture only a specified native window, without moving focus or reading desktop."""
import ctypes
from ctypes import wintypes as w
import sys
from PIL import Image
u=ctypes.windll.user32;g=ctypes.windll.gdi32
u.GetDC.argtypes=[w.HWND];u.GetDC.restype=w.HDC
u.GetWindowRect.argtypes=[w.HWND,ctypes.POINTER(w.RECT)]
u.PrintWindow.argtypes=[w.HWND,w.HDC,w.UINT]
u.ReleaseDC.argtypes=[w.HWND,w.HDC]
g.CreateCompatibleDC.argtypes=[w.HDC];g.CreateCompatibleDC.restype=w.HDC
g.CreateCompatibleBitmap.argtypes=[w.HDC,ctypes.c_int,ctypes.c_int];g.CreateCompatibleBitmap.restype=w.HBITMAP
g.SelectObject.argtypes=[w.HDC,w.HGDIOBJ];g.SelectObject.restype=w.HGDIOBJ
g.DeleteObject.argtypes=[w.HGDIOBJ];g.DeleteDC.argtypes=[w.HDC]
g.GetDIBits.argtypes=[w.HDC,w.HBITMAP,w.UINT,w.UINT,ctypes.c_void_p,ctypes.c_void_p,w.UINT]
hwnd=int(sys.argv[1]);r=w.RECT();assert u.GetWindowRect(hwnd,ctypes.byref(r))
width=r.right-r.left;height=r.bottom-r.top
hdc=u.GetDC(hwnd);mem=g.CreateCompatibleDC(hdc);bitmap=g.CreateCompatibleBitmap(hdc,width,height);old=g.SelectObject(mem,bitmap)
class Header(ctypes.Structure):
    _fields_=[('size',w.DWORD),('width',w.LONG),('height',w.LONG),('planes',w.WORD),('bits',w.WORD),('compression',w.DWORD),('image_size',w.DWORD),('x',w.LONG),('y',w.LONG),('used',w.DWORD),('important',w.DWORD)]
try:
    assert u.PrintWindow(hwnd,mem,2)
    header=Header(ctypes.sizeof(Header),width,-height,1,32,0,0,0,0,0,0);pixels=ctypes.create_string_buffer(width*height*4)
    assert g.GetDIBits(mem,bitmap,0,height,pixels,ctypes.byref(header),0)
    Image.frombuffer('RGB',(width,height),pixels,'raw','BGRX',0,1).save(sys.argv[2])
finally:
    g.SelectObject(mem,old);g.DeleteObject(bitmap);g.DeleteDC(mem);u.ReleaseDC(hwnd,hdc)
