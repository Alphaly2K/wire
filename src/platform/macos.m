// Public AppKit/CoreGraphics CPU frame presenter. No executable guest code.
#import <AppKit/AppKit.h>
#import <QuartzCore/CAMetalLayer.h>
#include <stdint.h>
#include <pthread.h>

typedef struct { uint32_t hwnd, message, wparam, lparam; } WireEvent;
static WireEvent events[512];
static unsigned event_head, event_tail;
static void queue(uint32_t hwnd, uint32_t msg, uint32_t wp, uint32_t lp) {
    unsigned next=(event_tail+1)%512;
    if(next==event_head) return;
    events[event_tail]=(WireEvent){hwnd,msg,wp,lp}; event_tail=next;
}
typedef struct { uint8_t vk, scan, extended; } WireKey;
// Virtual keys follow physical positions; text still uses the active host layout.
static WireKey key_info(unsigned code) {
    static const WireKey keys[128]={
        [0]={0x41,0x1e,0},[1]={0x53,0x1f,0},[2]={0x44,0x20,0},[3]={0x46,0x21,0},
        [4]={0x48,0x23,0},[5]={0x47,0x22,0},[6]={0x5a,0x2c,0},[7]={0x58,0x2d,0},
        [8]={0x43,0x2e,0},[9]={0x56,0x2f,0},[10]={0xe2,0x56,0},[11]={0x42,0x30,0},
        [12]={0x51,0x10,0},[13]={0x57,0x11,0},[14]={0x45,0x12,0},[15]={0x52,0x13,0},
        [16]={0x59,0x15,0},[17]={0x54,0x14,0},[18]={0x31,0x02,0},[19]={0x32,0x03,0},
        [20]={0x33,0x04,0},[21]={0x34,0x05,0},[22]={0x36,0x07,0},[23]={0x35,0x06,0},
        [24]={0xbb,0x0d,0},[25]={0x39,0x0a,0},[26]={0x37,0x08,0},[27]={0xbd,0x0c,0},
        [28]={0x38,0x09,0},[29]={0x30,0x0b,0},[30]={0xdd,0x1b,0},[31]={0x4f,0x18,0},
        [32]={0x55,0x16,0},[33]={0xdb,0x1a,0},[34]={0x49,0x17,0},[35]={0x50,0x19,0},
        [36]={0x0d,0x1c,0},[37]={0x4c,0x26,0},[38]={0x4a,0x24,0},[39]={0xde,0x28,0},
        [40]={0x4b,0x25,0},[41]={0xba,0x27,0},[42]={0xdc,0x2b,0},[43]={0xbc,0x33,0},
        [44]={0xbf,0x35,0},[45]={0x4e,0x31,0},[46]={0x4d,0x32,0},[47]={0xbe,0x34,0},
        [48]={0x09,0x0f,0},[49]={0x20,0x39,0},[50]={0xc0,0x29,0},[51]={0x08,0x0e,0},
        [53]={0x1b,0x01,0},[54]={0x5c,0x5c,1},[55]={0x5b,0x5b,1},
        [56]={0x10,0x2a,0},[57]={0x14,0x3a,0},[58]={0x12,0x38,0},[59]={0x11,0x1d,0},
        [60]={0x10,0x36,0},[61]={0x12,0x38,1},[62]={0x11,0x1d,1},
        [64]={0x80,0x68,0},[65]={0x6e,0x53,0},[67]={0x6a,0x37,0},[69]={0x6b,0x4e,0},
        [71]={0x90,0x45,0},[75]={0x6f,0x35,1},[76]={0x0d,0x1c,1},[78]={0x6d,0x4a,0},
        [79]={0x81,0x69,0},[80]={0x82,0x6a,0},[81]={0x92,0x59,0},
        [82]={0x60,0x52,0},[83]={0x61,0x4f,0},[84]={0x62,0x50,0},[85]={0x63,0x51,0},
        [86]={0x64,0x4b,0},[87]={0x65,0x4c,0},[88]={0x66,0x4d,0},[89]={0x67,0x47,0},
        [90]={0x83,0x6b,0},[91]={0x68,0x48,0},[92]={0x69,0x49,0},[96]={0x74,0x3f,0},[97]={0x75,0x40,0},
        [98]={0x76,0x41,0},[99]={0x72,0x3d,0},[100]={0x77,0x42,0},[101]={0x78,0x43,0},
        [103]={0x7a,0x57,0},[105]={0x7c,0x64,0},[106]={0x7f,0x67,0},[107]={0x7d,0x65,0},
        [109]={0x79,0x44,0},[111]={0x7b,0x58,0},[113]={0x7e,0x66,0},[114]={0x2d,0x52,1},
        [115]={0x24,0x47,1},[116]={0x21,0x49,1},[117]={0x2e,0x53,1},
        [118]={0x73,0x3e,0},[119]={0x23,0x4f,1},[120]={0x71,0x3c,0},
        [121]={0x22,0x51,1},[122]={0x70,0x3b,0},[123]={0x25,0x4b,1},
        [124]={0x27,0x4d,1},[125]={0x28,0x50,1},[126]={0x26,0x48,1},
    };
    return code<128?keys[code]:(WireKey){0};
}
@interface WireView : NSView <NSWindowDelegate> {
    BOOL _pressed[128];
    BOOL _keyActive;
    uint32_t _buttons;
    uint32_t _lastPoint;
    double _wheelX, _wheelY;
}
@property uint32_t hwnd;
@property unsigned logicalWidth, logicalHeight;
@property(nonatomic) CGImageRef image;
@property CAMetalLayer *gpuLayer;
@end
@implementation WireView
- (BOOL)isFlipped { return YES; }
- (BOOL)acceptsFirstResponder { return YES; }
- (BOOL)acceptsFirstMouse:(NSEvent *)event { (void)event; return YES; }
- (void)setImage:(CGImageRef)image {
    if(image) CGImageRetain(image);
    if(_image) CGImageRelease(_image);
    _image=image; [self setNeedsDisplay:YES];
}
- (void)dealloc { if(_image) CGImageRelease(_image); }
- (void)drawRect:(NSRect)dirty {
    (void)dirty;
    if(_gpuLayer) return;
    [[NSColor blackColor] setFill]; NSRectFill(self.bounds);
    if(!_image) return;
    CGContextRef context=NSGraphicsContext.currentContext.CGContext;
    CGContextSaveGState(context);
    CGContextSetInterpolationQuality(context,kCGInterpolationNone);
    CGContextTranslateCTM(context,0,self.bounds.size.height);
    CGContextScaleCTM(context,1,-1);
    CGContextDrawImage(context,NSRectToCGRect(self.bounds),_image);
    CGContextRestoreGState(context);
}
- (BOOL)windowShouldClose:(NSWindow *)sender {
    (void)sender; queue(_hwnd,0x10,0,0); return NO;
}
- (uint32_t)point:(NSEvent *)event {
    NSPoint p=[self convertPoint:event.locationInWindow fromView:nil];
    if(self.bounds.size.width<=0 || self.bounds.size.height<=0) return _lastPoint;
    int x=(int)floor(p.x*_logicalWidth/self.bounds.size.width);
    int y=(int)floor(p.y*_logicalHeight/self.bounds.size.height);
    _lastPoint=((uint32_t)y&65535)<<16|((uint32_t)x&65535);
    return _lastPoint;
}
- (uint32_t)mouseFlags:(NSEvent *)e {
    return _buttons | ((e.modifierFlags&NSEventModifierFlagShift)?4:0)
                    | ((e.modifierFlags&NSEventModifierFlagControl)?8:0);
}
- (void)button:(NSEvent *)e down:(BOOL)down number:(unsigned)n {
    static const uint32_t masks[]={1,2,16,32,64};
    if(n>=5) return;
    if(down) _buttons|=masks[n]; else _buttons&=~masks[n];
    uint32_t msg=n<3?0x201+n*3:0x20b;
    queue(_hwnd,msg+(down?0:1),[self mouseFlags:e]|(n>=3?(n-2)<<16:0),[self point:e]);
}
- (void)mouseDown:(NSEvent *)e {[self button:e down:YES number:0];}
- (void)mouseUp:(NSEvent *)e {[self button:e down:NO number:0];}
- (void)rightMouseDown:(NSEvent *)e {[self button:e down:YES number:1];}
- (void)rightMouseUp:(NSEvent *)e {[self button:e down:NO number:1];}
- (void)otherMouseDown:(NSEvent *)e {[self button:e down:YES number:(unsigned)e.buttonNumber];}
- (void)otherMouseUp:(NSEvent *)e {[self button:e down:NO number:(unsigned)e.buttonNumber];}
- (void)mouseMoved:(NSEvent *)e {queue(_hwnd,0x200,[self mouseFlags:e],[self point:e]);}
- (void)mouseDragged:(NSEvent *)e {[self mouseMoved:e];}
- (void)rightMouseDragged:(NSEvent *)e {[self mouseMoved:e];}
- (void)otherMouseDragged:(NSEvent *)e {[self mouseMoved:e];}
- (void)scrollWheel:(NSEvent *)e {
    // Precise deltas are pixels; ordinary wheel deltas are detents. Keep the
    // fractional part so slow trackpad movement is not discarded on each poll.
    double scale=e.hasPreciseScrollingDeltas?1.:120.;
    _wheelY+=e.scrollingDeltaY*scale;
    _wheelX-=e.scrollingDeltaX*scale;
    double *axes[]={&_wheelY,&_wheelX};
    for(unsigned n=0;n<2;n++) {
        double *axis=axes[n];
        if(isfinite(*axis)) *axis=fmax(-262144.,fmin(262136.,*axis));
        while(isfinite(*axis) && fabs(*axis)>=1.) {
            int delta=(int)fmax(-32768.,fmin(32767.,trunc(*axis)));
            *axis-=delta;
            queue(_hwnd,n?0x20e:0x20a,[self mouseFlags:e]|((uint32_t)(uint16_t)delta<<16),[self point:e]);
        }
        if(!isfinite(*axis)) *axis=0;
    }
}
- (uint32_t)keyCode:(unsigned)code down:(BOOL)down repeat:(BOOL)repeat alt:(BOOL)alt {
    WireKey key=key_info(code); if(!key.vk) return 0;
    BOOL previous=_pressed[code];
    uint32_t lp=1|((uint32_t)key.scan<<16)|((uint32_t)key.extended<<24)
                  |(alt?(1u<<29):0)|((previous||repeat||!down)?(1u<<30):0)|(!down?(1u<<31):0);
    _pressed[code]=down;
    BOOL system=alt||key.vk==0x12||key.vk==0x79;
    queue(_hwnd,(system?0x104:0x100)+(down?0:1),key.vk,lp);
    return lp;
}
- (void)keyDown:(NSEvent *)e {
    BOOL alt=(e.modifierFlags&NSEventModifierFlagOption)!=0;
    uint32_t lp=[self keyCode:e.keyCode down:YES repeat:e.isARepeat alt:alt];
    if(!lp) return;
    NSString *s=e.characters;
    for(NSUInteger i=0;i<s.length;i++) {
        unichar c=[s characterAtIndex:i];
        if(c>=0xf700 && c<=0xf8ff) continue;
        if(c==0x7f) c=8;
        if(c==3 && e.keyCode==76) c=13;
        queue(_hwnd,alt?0x106:0x102,c,lp);
    }
}
- (void)keyUp:(NSEvent *)e {
    [self keyCode:e.keyCode down:NO repeat:NO alt:(e.modifierFlags&NSEventModifierFlagOption)!=0];
}
- (void)flagsChanged:(NSEvent *)e {
    unsigned code=e.keyCode; if(code>=128) return;
    if(code==57) {
        [self keyCode:code down:YES repeat:NO alt:NO];
        [self keyCode:code down:NO repeat:NO alt:NO];
        return;
    }
    NSEventModifierFlags mask;
    switch(code) {
        case 56:case 60:mask=NSEventModifierFlagShift;break;
        case 59:case 62:mask=NSEventModifierFlagControl;break;
        case 58:case 61:mask=NSEventModifierFlagOption;break;
        case 54:case 55:mask=NSEventModifierFlagCommand;break;
        default:return;
    }
    BOOL alt=_pressed[58]||_pressed[61];
    [self keyCode:code down:(e.modifierFlags&mask)&&!_pressed[code] repeat:NO alt:alt];
}
- (void)windowDidBecomeKey:(NSNotification *)note {
    (void)note;
    if(_keyActive) return;
    _keyActive=YES;
    queue(_hwnd,0x1c,1,0);
    queue(_hwnd,6,1,0);
    queue(_hwnd,7,0,0);
}
- (void)windowDidResignKey:(NSNotification *)note {
    (void)note;
    // A release outside the window must not leave guest keys or buttons held.
    for(unsigned code=0;code<128;code++) if(_pressed[code]) {
        [self keyCode:code down:NO repeat:NO alt:_pressed[58]||_pressed[61]];
    }
    const uint32_t masks[]={1,2,16,32,64};
    for(unsigned n=0;n<5;n++) if(_buttons&masks[n]) {
        _buttons&=~masks[n];
        queue(_hwnd,n<3?0x202+n*3:0x20c,_buttons|(n>=3?(n-2)<<16:0),_lastPoint);
    }
    _wheelX=_wheelY=0;
    if(_keyActive) {
        _keyActive=NO;
        queue(_hwnd,8,0,0);
        queue(_hwnd,6,0,0);
        queue(_hwnd,0x1c,0,0);
    }
}
@end
static NSMutableDictionary<NSNumber *,NSWindow *> *windows;
int wire_ui_init(void) {
    if(!NSThread.isMainThread) return 0;
    // Game startup and interpretation are user-initiated foreground work.
    // Let the public scheduler account for that latency requirement.
    pthread_set_qos_class_self_np(QOS_CLASS_USER_INITIATED,0);
    @autoreleasepool {
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
        [NSApp finishLaunching];
        windows=[NSMutableDictionary new];
        event_head=event_tail=0;
        return 1;
    }
}
void wire_ui_window(uint32_t hwnd,const char *title,unsigned width,unsigned height,int visible) {
    @autoreleasepool {
        NSWindow *w=windows[@(hwnd)];
        if(!visible) {if(w) [w orderOut:nil];return;}
        if(!w) {
            NSSize limit=NSScreen.mainScreen.visibleFrame.size;
            double scale=fmin(1.,fmin((limit.width-60)/width,(limit.height-100)/height));
            NSRect r=NSMakeRect(0,0,width*scale,height*scale);
            w=[[NSWindow alloc] initWithContentRect:r styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable|NSWindowStyleMaskMiniaturizable backing:NSBackingStoreBuffered defer:NO];
            w.releasedWhenClosed=NO;
            WireView *view=[[WireView alloc] initWithFrame:r];view.hwnd=hwnd;
            w.contentView=view;w.delegate=view;w.acceptsMouseMovedEvents=YES;
            [w makeFirstResponder:view];[w center];
            windows[@(hwnd)]=w;
            [NSApp activateIgnoringOtherApps:YES];
        }
        WireView *view=(WireView *)w.contentView;
        if(view.logicalWidth!=width||view.logicalHeight!=height) {
            NSSize limit=NSScreen.mainScreen.visibleFrame.size;
            double scale=fmin(1.,fmin((limit.width-60)/width,(limit.height-100)/height));
            [w setContentSize:NSMakeSize(width*scale,height*scale)];
        }
        view.logicalWidth=width;view.logicalHeight=height;
        w.title=[NSString stringWithUTF8String:title]?:@"wire";
        if(!w.visible) [w makeKeyAndOrderFront:nil];
    }
}
void wire_ui_remove(uint32_t hwnd) {
    @autoreleasepool {NSWindow *w=windows[@(hwnd)];w.delegate=nil;[w close];[windows removeObjectForKey:@(hwnd)];}
}
void wire_ui_window_topmost(uint32_t hwnd,int topmost) {
    @autoreleasepool {windows[@(hwnd)].level=topmost?NSFloatingWindowLevel:NSNormalWindowLevel;}
}
int wire_ui_frame(uint32_t hwnd,const unsigned char *rgba,unsigned width,unsigned height) {
    @autoreleasepool {
        NSWindow *w=windows[@(hwnd)];if(!w) return 0;
        CFDataRef data=CFDataCreate(kCFAllocatorDefault,rgba,(CFIndex)width*height*4);
        if(!data) return 0;
        CGDataProviderRef provider=CGDataProviderCreateWithCFData(data);
        CGColorSpaceRef color=CGColorSpaceCreateDeviceRGB();
        CGImageRef image=CGImageCreate(width,height,8,32,width*4,color,(CGBitmapInfo)kCGImageAlphaNoneSkipLast,provider,NULL,false,kCGRenderingIntentDefault);
        if(image) ((WireView *)w.contentView).image=image;
        if(image) CGImageRelease(image);CGColorSpaceRelease(color);CGDataProviderRelease(provider);CFRelease(data);
        return image!=NULL;
    }
}
// Retained public CAMetalLayer. Its owner must release it after destroying the
// Vulkan surface; removing an NSWindow cannot invalidate an outstanding surface.
void *wire_ui_metal_layer(uint32_t hwnd) {
    if(!NSThread.isMainThread) return NULL;
    @autoreleasepool {
        NSWindow *w=windows[@(hwnd)];if(!w || !w.visible) return NULL;
        WireView *view=(WireView *)w.contentView;
        if(!view.gpuLayer) {
            CAMetalLayer *layer=[CAMetalLayer layer];
            layer.opaque=YES;layer.framebufferOnly=NO;
            layer.contentsScale=w.backingScaleFactor;
            view.wantsLayer=YES;view.layer=layer;view.gpuLayer=layer;
        }
        // Metal drawable dimensions are backing pixels; guest input remains in
        // logical client pixels even when the window is scaled or on Retina.
        view.gpuLayer.contentsScale=w.backingScaleFactor;
        view.gpuLayer.drawableSize=[view convertSizeToBacking:view.bounds.size];
        return (__bridge_retained void *)view.gpuLayer;
    }
}
void wire_ui_release_metal_layer(void *layer) {
    if(layer) {CFRelease(layer);}
}
int wire_ui_poll(WireEvent *out) {
    @autoreleasepool {
        for(unsigned i=0;i<64;i++) {
            NSEvent *e=[NSApp nextEventMatchingMask:NSEventMaskAny untilDate:NSDate.distantPast inMode:NSDefaultRunLoopMode dequeue:YES];
            if(!e) break;[NSApp sendEvent:e];
        }
        // Drain ready run-loop sources without adding a 1 ms sleep to every
        // interpreter poll. Guest waits supply their own deadlines.
        CFRunLoopRunInMode(kCFRunLoopDefaultMode,0,true);
        [NSApp updateWindows];
        if(event_head==event_tail) return 0;
        *out=events[event_head];event_head=(event_head+1)%512;return 1;
    }
}
void wire_ui_shutdown(void) {
    @autoreleasepool {for(NSWindow *w in windows.allValues){w.delegate=nil;[w close];} windows=nil;}
}
