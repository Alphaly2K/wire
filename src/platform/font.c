// Public CoreText font realization and metrics. No guest pointers enter here.
#include <CoreText/CoreText.h>
#include <stdint.h>
#include <math.h>
#include <string.h>
#include <stdlib.h>
static int max_int(int a,int b) {return a>b?a:b;}
static uint16_t be16(const uint8_t *b) { return ((uint16_t)b[0]<<8)|b[1]; }
int wire_font_metrics(const uint16_t *face,unsigned length,int height,int width,int weight,
    int italic,int32_t *values,uint16_t *characters,uint8_t *pitch) {
    if(height < -4096 || height > 4096 || width<0 || width>4096 || length>32) return 0;
    CGFloat size=height?abs(height):16;
    CFStringRef name=CFStringCreateWithCharacters(NULL,face,length);
    CTFontRef font=name?CTFontCreateWithName(name,size,NULL):NULL;
    if(name) CFRelease(name);
    if(!font) return 0;
    CTFontSymbolicTraits traits=(weight>=600?kCTFontBoldTrait:0)|(italic?kCTFontItalicTrait:0);
    if(traits) { CTFontRef changed=CTFontCreateCopyWithSymbolicTraits(font,0,NULL,traits,traits);
        if(changed) { CFRelease(font);font=changed; } }
    if(height>0) {
        CGFloat cell=CTFontGetAscent(font)+CTFontGetDescent(font);
        CTFontRef scaled=CTFontCreateCopyWithAttributes(font,size*height/cell,NULL,NULL);
        if(!scaled) {CFRelease(font);return 0;} CFRelease(font);font=scaled;
    }
    CGFloat units=CTFontGetUnitsPerEm(font),point=CTFontGetSize(font);
    CGFloat average=0,maximum=0;
    CFDataRef os2=CTFontCopyTable(font,kCTFontTableOS2,kCTFontTableOptionNoOptions);
    if(os2 && CFDataGetLength(os2)>=4) average=(int16_t)be16(CFDataGetBytePtr(os2)+2)*point/units;
    if(os2) CFRelease(os2);
    CFDataRef hhea=CTFontCopyTable(font,kCTFontTableHhea,kCTFontTableOptionNoOptions);
    if(hhea && CFDataGetLength(hhea)>=12) maximum=be16(CFDataGetBytePtr(hhea)+10)*point/units;
    if(hhea) CFRelease(hhea);
    if(average<=0 || maximum<=0) {UniChar text[2]={'x','W'};CGGlyph glyphs[2];CGSize advance[2];
        CTFontGetGlyphsForCharacters(font,text,glyphs,2);CTFontGetAdvancesForGlyphs(font,kCTFontOrientationHorizontal,glyphs,advance,2);
        if(average<=0) average=advance[0].width;if(maximum<=0) maximum=advance[1].width;}
    int ascent=(int)ceil(CTFontGetAscent(font)),descent=(int)ceil(CTFontGetDescent(font));
    values[0]=ascent+descent;values[1]=ascent;values[2]=descent;
    values[3]=max_int(0,values[0]-(int)lround(point));values[4]=(int)ceil(CTFontGetLeading(font));
    CGFloat scale=width && average>0?width/average:1;
    values[5]=max_int(1,(int)lround(average*scale));values[6]=max_int(values[5],(int)ceil(maximum*scale));
    values[7]=weight?weight:400;values[8]=0;values[9]=96;values[10]=96;
    CFCharacterSetRef charset=CTFontCopyCharacterSet(font);uint16_t first=0,last=0;
    if(charset) {for(unsigned i=0;i<=65535;i++) if(CFCharacterSetIsCharacterMember(charset,(UniChar)i)) {first=(uint16_t)i;break;}
        for(int i=65535;i>=0;i--) if(CFCharacterSetIsCharacterMember(charset,(UniChar)i)) {last=(uint16_t)i;break;}CFRelease(charset);}
    characters[0]=first;characters[1]=last;characters[2]='?';characters[3]=' ';
    *pitch=6|((CTFontGetSymbolicTraits(font)&kCTFontMonoSpaceTrait)?0:1);
    CFRelease(font);return 1;
}
