"""Versioned original AMD64 V2 acceptance fixtures.

Moved from the 2026-10-09 audit without changing guest bytes or expected state.
The adjacent golden manifest records both smoke and 100M-work inputs.
"""
import struct

from amd64_perf import pe64
from interpreter_perf import additional


def fixture(name,work):
    if name=='sse-register':return additional(name,work)
    c=bytearray();listing=[];n=0
    def emit(h,asm):
        listing.append(dict(offset=len(c),asm=asm));c.extend(bytes.fromhex(h))
    def imm(h,val,asm,size=8):emit(h+int(val).to_bytes(size,'little').hex(),asm)
    if name.startswith('code-'):
        n=max(1,work//12);a=0x1000;b=0x21002+(0x40 if name=='code-control' else 0)
        emit('4831c0','xor rax,rax');emit('4831c9','xor rcx,rcx')
        emit('488dbc2400ffffff','lea rdi,[rsp-256]')
        imm('48be',0x140001000+a,'mov rsi,function A');imm('48bb',0x140001000+b,'mov rbx,function B')
        imm('41bf',n,'mov r15d,N',4);loop=len(c)
        emit('ffd6','call rsi');emit('ffd3','call rbx')
        epilogue='48034708';expect=4*n;count=12*n+8
    elif name.startswith('data-'):
        n=max(1,work//6);gap=0x40000 if name=='data-alias' else 0x1000
        emit('4831c0','xor rax,rax');emit('488db42400f0ffff','lea rsi,[rsp-4096]')
        emit('488dbc24'+struct.pack('<i',-4096-gap).hex(),'lea rdi,[rsp-4096-gap]')
        emit('48c70601000000','mov qword [rsi],1');emit('48c70703000000','mov qword [rdi],3')
        imm('41bf',n,'mov r15d,N',4);loop=len(c)
        emit('480306 480307 480306 480307','add rax,[rsi]; add rax,[rdi]; repeat once')
        epilogue='';expect=8*n;count=6*n+7
    elif name=='fp-scalar':
        n=max(1,work//9)
        for bits,modrm,label in [(0x3fc00000,'c0','xmm0=float 1.5'),(0x3f800000,'c8','xmm1=float 1'),
                                 (0x4000000000000000,'d0','xmm2=double 2'),(0x3ff0000000000000,'d8','xmm3=double 1')]:
            imm('48b8',bits,'mov rax,bits');emit('66480f6e'+modrm,'movq '+label+',rax')
        emit('660fefe4','pxor xmm4,xmm4');emit('4531e4','xor r12d,r12d');imm('41bf',n,'mov r15d,N',4);loop=len(c)
        emit('f30f59c1','mulss xmm0,xmm1');emit('f20f59d3','mulsd xmm2,xmm3');emit('f20f58d4','addsd xmm2,xmm4')
        emit('f30f5ae8','cvtss2sd xmm5,xmm0');emit('f20f5af5','cvtsd2ss xmm6,xmm5');emit('f30f2cc6','cvttss2si eax,xmm6')
        emit('4901c4','add r12,rax');epilogue='4c89e0';expect=n;count=9*n+13
    else:raise ValueError(name)
    emit('49ffcf','dec r15');at=len(c);emit('0f85'+struct.pack('<i',loop-at-6).hex(),'jne loop')
    if epilogue:emit(epilogue,'checksum return preparation')
    emit('c3','ret')
    if name.startswith('code-'):
        for off in [a,b]:
            c.extend(bytes(off-len(c)))
            emit('4883c001 48894708 4883c101 c3','add rax,1; mov [rdi+8],rax; add rcx,1; ret')
    image=pe64(c)
    if name.startswith('data-'):struct.pack_into('<Q',image,0x98+72,1024*1024)
    return image,dict(rax=hex(expect),retired=count,work=count,iterations=n,listing=listing)
