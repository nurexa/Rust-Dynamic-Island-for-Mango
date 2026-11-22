import sys, struct, zlib
def png(path):
    d=open(path,'rb').read(); p=8; idat=b''; 
    while p<len(d):
        l,t=struct.unpack('>I4s',d[p:p+8]); c=d[p+8:p+8+l]; p+=12+l
        if t==b'IHDR': w,h,bd,ct=struct.unpack('>IIBB',c[:10])
        if t==b'IDAT': idat+=c
    raw=zlib.decompress(idat); bpp={2:3,6:4}[ct]; st=w*bpp; out=bytearray(); prev=bytes(st); i=0
    for y in range(h):
        f=raw[i]; line=bytearray(raw[i+1:i+1+st]); i+=1+st
        for x in range(st):
            a=line[x-bpp] if x>=bpp else 0; b=prev[x]; c=prev[x-bpp] if x>=bpp else 0
            if f==1: line[x]=(line[x]+a)&255
            elif f==2: line[x]=(line[x]+b)&255
            elif f==3: line[x]=(line[x]+(a+b)//2)&255
            elif f==4:
                q=a+b-c; pa,pb,pc=abs(q-a),abs(q-b),abs(q-c); pr=a if pa<=pb and pa<=pc else b if pb<=pc else c
                line[x]=(line[x]+pr)&255
        out+=line; prev=bytes(line)
    return w,h,bpp,out
w,h,bpp,o=png(sys.argv[1]); x,y=int(sys.argv[2]),int(sys.argv[3])
def island(y):
    """One of the dark island's layers: well, background, card, tint. They're neutral to
    faintly blue. The test window is magenta (less green than red) and the desktop teal."""
    r,g,b=o[(y*w+x)*bpp:(y*w+x)*bpp+3]
    return max(r,g,b)<=80 and max(r,g,b)-min(r,g,b)<=22 and g+3>=r
r,g,b=o[(y*w+x)*bpp:(y*w+x)*bpp+3]
# "height": how far the opaque panel runs straight down from (x, y).
def px(yy): return tuple(o[(yy*w+x)*bpp:(yy*w+x)*bpp+3])
# "span": the first and last row of the run of this pixel's colour through (x, y): a card's face, top to bottom.
if len(sys.argv)>4 and sys.argv[4]=='span':
    t=y
    while t>0 and px(t-1)==px(y): t-=1
    b=y
    while b<h-1 and px(b+1)==px(y): b+=1
    print(t,b)
elif len(sys.argv)>4 and sys.argv[4]=='height': print(next((i for i in range(h-y) if not island(y+i)), h-y))
elif len(sys.argv)>4: print('yes' if island(y) else 'no')
else: print(r,g,b)
