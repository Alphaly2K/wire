// Fixed-function D3D9 rasterization. This is a static host graphics program;
// it never contains translated guest instructions or accesses guest memory.
#include <metal_stdlib>
using namespace metal;
struct Vertex { float4 p, uv, color; };
struct Triangle { Vertex v[3]; };
struct Params {
    uint width, height, tw, th, texture_format, target_format, depth_width, count;
    int clip[4];
    uint address[2], filter[2], r[175], stage[7], mask, has_depth;
};
float edge(Vertex a, Vertex b, float2 xy) {
    return (b.p.x-a.p.x)*(xy.y-a.p.y)-(b.p.y-a.p.y)*(xy.x-a.p.x);
}
bool owned(Vertex a, Vertex b) { return b.p.y<a.p.y || (b.p.y==a.p.y && b.p.x>a.p.x); }
bool compare(uint f, uint a, uint b) {
    switch(f) { case 1:return false;case 2:return a<b;case 3:return a==b;
        case 4:return a<=b;case 5:return a>b;case 6:return a!=b;
        case 7:return a>=b;default:return true; }
}
uint stencil(uint op,uint old,uint ref,uint mask) {
    uint n;
    switch(op) {case 1:n=old;break;case 2:n=0;break;case 3:n=ref;break;
        case 4:n=min(old+1,255u);break;case 5:n=old?old-1:0;break;
        case 6:n=~old;break;case 7:n=old+1;break;default:n=old-1;}
    return (old&~mask)|(n&mask&255);
}
uint4 channels(uint p) {return uint4(p&255,(p>>8)&255,(p>>16)&255,p>>24);}
uint packed(uint4 p) {return p.x|(p.y<<8)|(p.z<<16)|(p.w<<24);}
uint modulate(uint a,uint b) {return packed((channels(a)*channels(b)+127)/255);}
uint combine(uint op,uint a,uint b,uint tex,uint diffuse) {
    uint first=a==2?tex:diffuse,second=b==2?tex:diffuse;
    return op==2?first:op==3?second:modulate(first,second);
}
uint factor(uint f,uint alpha) {return f==1?0:f==2?255:f==5?alpha:255-alpha;}
uint blend(uint src,uint dst,uint sf,uint df) {
    return packed(min((channels(src)*factor(sf,src>>24)+channels(dst)*factor(df,src>>24)+127)/255,255u));
}
int texel_coordinate(int i,uint size,uint mode) {
    if(mode!=1) return clamp(i,0,int(size)-1);
    int n=i%int(size);return n<0?n+int(size):n;
}
uint texel(device const uint *src,constant Params &p,int x,int y) {
    uint pixel=src[texel_coordinate(y,p.th,p.address[1])*p.tw+texel_coordinate(x,p.tw,p.address[0])];
    return p.texture_format==22?pixel|0xff000000:pixel;
}
uint sample_pixel(device const uint *src,constant Params &p,float2 uv) {
    for(uint i=0;i<2;i++) uv[i]=p.address[i]==1?uv[i]-floor(uv[i]):clamp(uv[i],0.0f,1.0f);
    float2 at=uv*float2(p.tw,p.th);
    if(p.filter[0]==1) return texel(src,p,int(floor(at.x)),int(floor(at.y)));
    at-=0.5f;
    int2 base=int2(floor(at));float2 f=at-floor(at);
    float4 c00=float4(channels(texel(src,p,base.x,base.y)));
    float4 c10=float4(channels(texel(src,p,base.x+1,base.y)));
    float4 c01=float4(channels(texel(src,p,base.x,base.y+1)));
    float4 c11=float4(channels(texel(src,p,base.x+1,base.y+1)));
    float4 value=c00*((1-f.x)*(1-f.y))+c10*(f.x*(1-f.y))
        +c01*((1-f.x)*f.y)+c11*(f.x*f.y);
    return packed(uint4(floor(clamp(value,0.0f,255.0f)+0.5f)));
}
// Validated PS2 records are data interpreted by this fixed host program.
float4 rgba(uint pixel) { return float4((pixel>>16)&255,(pixel>>8)&255,pixel&255,pixel>>24)/255.0f; }
uint shader_texel(device const uint *src,uint tw,uint th,uint format,uint2 address,int x,int y) {
    uint pixel=src[texel_coordinate(y,th,address.y)*tw+texel_coordinate(x,tw,address.x)];
    return format==22?pixel|0xff000000:pixel;
}
float4 sample_shader(device const uint *src,constant Params &p,device const uint *program,uint slot,float2 uv) {
    uint tw=slot?program[1]:p.tw,th=slot?program[2]:p.th,format=slot?program[3]:p.texture_format;
    uint2 address=slot?uint2(program[4],program[5]):uint2(p.address[0],p.address[1]);
    uint filter=slot?program[6]:p.filter[0];src+=slot?program[7]:0;
    if(!all(isfinite(uv))) return float4(0);
    for(uint i=0;i<2;i++) uv[i]=address[i]==1?uv[i]-floor(uv[i]):clamp(uv[i],0.0f,1.0f);
    float2 at=uv*float2(tw,th);
    if(filter==1) return rgba(shader_texel(src,tw,th,format,address,int(floor(at.x)),int(floor(at.y))));
    at-=0.5f;int2 base=int2(floor(at));float2 f=at-floor(at);
    return rgba(shader_texel(src,tw,th,format,address,base.x,base.y))*((1-f.x)*(1-f.y))
        +rgba(shader_texel(src,tw,th,format,address,base.x+1,base.y))*(f.x*(1-f.y))
        +rgba(shader_texel(src,tw,th,format,address,base.x,base.y+1))*((1-f.x)*f.y)
        +rgba(shader_texel(src,tw,th,format,address,base.x+1,base.y+1))*(f.x*f.y);
}
uint reg_kind(uint t) { return ((t>>28)&7)|((t>>8)&24); }
float4 ps_source(uint t,thread float4 *regs,device const uint *program,float4 diffuse,float4 uv,float4 uv1) {
    uint n=t&2047,k=reg_kind(t);
    float4 v=k==0?regs[n]:k==1?diffuse:k==2?as_type<float4>(uint4(program[9+n*4],program[10+n*4],program[11+n*4],program[12+n*4])):n==0?uv:uv1;
    v=float4(v[(t>>16)&3],v[(t>>18)&3],v[(t>>20)&3],v[(t>>22)&3]);
    uint m=(t>>24)&15;return m==1?-v:m==11?abs(v):m==12?-abs(v):v;
}
bool run_pixel_shader(device const uint *program,device const uint *src,constant Params &p,float4 diffuse,float4 uv,float4 uv1,thread uint &color) {
    float4 regs[12];for(uint i=0;i<12;i++) regs[i]=float4(0);
    float4 out=float4(0);
    for(uint i=0;i<program[0];i++) {
        device const uint *op=program+137+i*8;uint code=op[0]&65535,d=op[1];
        if(code==65) {if(any(regs[d&2047].xyz<0)) return false;continue;}
        float4 a=ps_source(op[2],regs,program,diffuse,uv,uv1),b=ps_source(op[3],regs,program,diffuse,uv,uv1),c=ps_source(op[4],regs,program,diffuse,uv,uv1),v;
        switch(code) {
            case 1:v=a;break;case 2:v=a+b;break;case 3:v=a-b;break;
            case 4:v=a*b+c;break;case 5:v=a*b;break;case 6:v=float4(1/a.x);break;
            case 7:v=float4(1/sqrt(abs(a.x)));break;case 8:v=float4(dot(a.xyz,b.xyz));break;
            case 9:v=float4(dot(a,b));break;case 10:v=min(a,b);break;case 11:v=max(a,b);break;
            case 12:v=select(float4(0),float4(1),a<b);break;case 13:v=select(float4(0),float4(1),a>=b);break;
            case 14:v=float4(exp2(a.x));break;case 15:v=float4(log2(abs(a.x)));break;
            case 18:v=a*b+(1-a)*c;break;case 19:v=a-floor(a);break;
            case 32:v=float4(pow(abs(a.x),b.x));break;case 35:v=abs(a);break;
            case 36:v=float4(normalize(a.xyz),a.w);break;case 37:v=float4(cos(a.x),sin(a.x),0,0);break;
            case 66:v=sample_shader(src,p,program,op[3]&2047,(op[0]&65536)?a.xy/a.w:a.xy);break;
            case 88:v=select(c,b,a>=0);break;case 90:v=float4(a.x*b.x+a.y*b.y+c.x);break;
            default:return false;
        }
        if(d&1048576) v=clamp(v,0.0f,1.0f);
        for(uint j=0;j<4;j++) if(d&(1<<(16+j))) {if(reg_kind(d)==8) out[j]=v[j];else regs[d&2047][j]=v[j];}
    }
    for(uint i=0;i<4;i++) if(isnan(out[i])) out[i]=0;
    uint4 value=uint4(floor(clamp(out,0.0f,1.0f)*255.0f+0.5f));
    color=(value.w<<24)|(value.x<<16)|(value.y<<8)|value.z;return true;
}
kernel void raster(device const Triangle *triangles [[buffer(0)]],
    constant Params &p [[buffer(1)]],device const uint *src [[buffer(2)]],
    device uint *dest [[buffer(3)]],device uint *depth [[buffer(4)]],
    device const uint *program [[buffer(5)]],uint2 at [[thread_position_in_grid]]) {
    if(at.x>=p.width || at.y>=p.height || int(at.x)<p.clip[0] || int(at.y)<p.clip[1]
        || int(at.x)>=p.clip[2] || int(at.y)>=p.clip[3]) return;
    uint index=at.y*p.width+at.x,di=at.y*p.depth_width+at.x;
    uint dst=dest[index],old=p.has_depth?depth[di]:0;
    float2 xy=float2(at);
    for(uint i=0;i<p.count;i++) {
        Vertex a=triangles[i].v[0],b=triangles[i].v[1],c=triangles[i].v[2];
        float area=edge(a,b,c.p.xy);
        if(area==0 || (p.r[22]==2 && area>0) || (p.r[22]==3 && area<0)) continue;
        if(area<0) {Vertex tmp=b;b=c;c=tmp;area=-area;}
        float3 e=float3(edge(b,c,xy),edge(c,a,xy),edge(a,b,xy));
        if(e.x<0 || (e.x==0 && !owned(b,c)) || e.y<0 || (e.y==0 && !owned(c,a))
            || e.z<0 || (e.z==0 && !owned(a,b))) continue;
        float3 w=e/area,pw=w;
        bool affine=a.p.w==b.p.w && a.p.w==c.p.w;
        if(!affine) {
            pw*=float3(a.p.w,b.p.w,c.p.w);float sum=pw.x+pw.y+pw.z;
            if(sum<=0) continue;pw/=sum;
        }
        float2 uv;
        if(affine) {
            float2 da=b.uv.xy-a.uv.xy,db=c.uv.xy-a.uv.xy;
            float2 dx=(da*(c.p.y-a.p.y)-db*(b.p.y-a.p.y))/area;
            float2 dy=(db*(b.p.x-a.p.x)-da*(c.p.x-a.p.x))/area;
            uv=(a.uv.xy+dy*(xy.y-a.p.y))+dx*(xy.x-a.p.x);
        } else uv=a.uv.xy*pw.x+b.uv.xy*pw.y+c.uv.xy*pw.z;
        if(!all(isfinite(uv))) continue;
        float4 dc=all(a.color==b.color) && all(a.color==c.color)?a.color:
            a.color*pw.x+b.color*pw.y+c.color*pw.z;
        uint diffuse=packed(uint4(floor(clamp(dc,0.0f,255.0f)+0.5f)));
        uint tex=sample_pixel(src,p,uv);
        uint rgb=combine(p.stage[1],p.stage[2],p.stage[3],tex,diffuse);
        uint alpha=p.stage[4]==1?diffuse:combine(p.stage[4],p.stage[5],p.stage[6],tex,diffuse);
        uint color=(rgb&0xffffff)|(alpha&0xff000000);
        if(program[0] && !run_pixel_shader(program,src,p,dc.zyxw/255.0f,float4(uv,0,1),float4(a.uv.zw*pw.x+b.uv.zw*pw.y+c.uv.zw*pw.z,0,1),color)) continue;
        if(p.r[15] && !compare(p.r[25],color>>24,p.r[24])) continue;
        if(p.has_depth) {
            uint z=uint(floor(clamp(a.p.z*w.x+b.p.z*w.y+c.p.z*w.z,0.0f,1.0f)*16777215.0f+0.5f));
            bool st=p.r[52]!=0;
            bool sp=!st || compare(p.r[56],p.r[57]&p.r[58]&255,old&p.r[58]&255);
            bool dp=!p.r[7] || compare(p.r[23],z,old>>8);
            if(st) old=(old&0xffffff00)|stencil(p.r[!sp?53:!dp?54:55],old&255,p.r[57],p.r[59]&255);
            if(!sp || !dp) continue;
            if(p.r[7] && p.r[14]) old=(z<<8)|(old&255);
        }
        if(p.r[27]) color=blend(color,dst,p.r[19],p.r[20]);
        dst=(color&p.mask)|(dst&~p.mask);
        if(p.target_format==22) dst|=0xff000000;
    }
    dest[index]=dst;if(p.has_depth) depth[di]=old;
}
