#!/usr/bin/env python3
"""Drive a headless compositor's pointer through zwlr_virtual_pointer_v1.
   vp.py move X Y | down [btn] | up [btn] | click X Y [btn] | scroll N | path X0 Y0 X1 Y1 STEPS MS"""
import os, socket, struct, sys, time
W, H = int(os.environ.get("VP_W","1920")), 1080
s = socket.socket(socket.AF_UNIX); s.connect(os.path.join(os.environ["XDG_RUNTIME_DIR"], os.environ["WAYLAND_DISPLAY"]))
def msg(obj, op, payload=b""): s.send(struct.pack("<IHH", obj, op, 8+len(payload)) + payload)
def sstr(t):
    b=t.encode()+b"\0"; return struct.pack("<I",len(b))+b+b"\0"*((4-len(b)%4)%4)
msg(1,1,struct.pack("<I",2)); msg(1,0,struct.pack("<I",3))
buf=b""; g={}; done=False
while not done:
    buf+=s.recv(65536)
    while len(buf)>=8:
        o,op,sz=struct.unpack("<IHH",buf[:8])
        if len(buf)<sz: break
        m,buf=buf[8:sz],buf[sz:]
        if o==3: done=True
        if o==2 and op==0:
            n,l=struct.unpack("<II",m[:8]); i=m[8:8+l-1].decode(); v=struct.unpack("<I",m[8+((l+3)&~3):][:4])[0]; g[i]=(n,v)
n,v=g["zwlr_virtual_pointer_manager_v1"]
msg(2,0,struct.pack("<I",n)+sstr("zwlr_virtual_pointer_manager_v1")+struct.pack("<II",1,4))
msg(4,0,struct.pack("<II",0,5))  # create_virtual_pointer(seat=null) -> 5
t=[int(time.time()*1000)&0xffffffff]
def now():
    t[0]=(t[0]+1)&0xffffffff; return t[0]
def fr(): msg(5,4)
def mv(x,y): msg(5,1,struct.pack("<IIIII",now(),int(x),int(y),W,H)); fr()
def btn(b,st): msg(5,2,struct.pack("<III",now(),b,st)); fr()
def rel(dx,dy): msg(5,0,struct.pack("<Iii",now(),int(dx*256),int(dy*256))); fr()
B={"l":0x110,"r":0x111,"m":0x112}
def run(a):
    c=a[0]
    if c=="move": mv(float(a[1]),float(a[2]))
    elif c=="down": btn(B[a[1] if len(a)>1 else "l"],1)
    elif c=="up": btn(B[a[1] if len(a)>1 else "l"],0)
    elif c=="click":
        mv(float(a[1]),float(a[2])); time.sleep(.03); b=B[a[3] if len(a)>3 else "l"]; btn(b,1); time.sleep(.03); btn(b,0)
    elif c=="dbl":
        mv(float(a[1]),float(a[2]))
        for _ in range(2):
            time.sleep(.05); btn(B["l"],1); time.sleep(.03); btn(B["l"],0)
    elif c=="scroll":
        msg(5,3,struct.pack("<IIi",now(),0,int(15*256*int(a[1])))); fr()
    elif c=="drag":
        dx,dy,steps,ms=float(a[1]),float(a[2]),int(a[3]),int(a[4])
        btn(B["l"],1); time.sleep(.03)
        for i in range(steps):
            rel(dx/steps,dy/steps); time.sleep(ms/1000)
        time.sleep(.03); btn(B["l"],0)
    elif c=="path":
        x0,y0,x1,y1,st,ms=float(a[1]),float(a[2]),float(a[3]),float(a[4]),int(a[5]),int(a[6])
        for i in range(st+1):
            f=i/st; mv(x0+(x1-x0)*f,y0+(y1-y0)*f); time.sleep(ms/1000)
# Daemon: the seat only has a pointer while this object lives.
fifo=sys.argv[1]
mv(1500,900)
while True:
    with open(fifo) as f:
        for line in f:
            if line.strip()=="exit": sys.exit()
            if line.strip(): run(line.split())
