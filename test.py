import socket, struct

def rf(s,n):
    b=b''
    while len(b)<n:
        c=s.recv(n-len(b))
        if not c: raise ConnectionError
        b+=c
    return b

s=socket.socket()
s.connect(('127.0.0.1',1234))

p=struct.pack('<I',3)
p+=struct.pack('<I',3)+b'set'
p+=struct.pack('<I',1)+b'k'
p+=struct.pack('<I',1)+b'v'
s.send(struct.pack('<I',len(p))+p)

n=struct.unpack('<I',rf(s,4))[0]
print('resp:', rf(s,n).hex())
s.close()
