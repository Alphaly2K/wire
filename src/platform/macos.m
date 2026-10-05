// Public AppKit/CoreGraphics CPU frame presenter. No executable guest code.
#import <AppKit/AppKit.h>
#include <stdint.h>

typedef struct { uint32_t hwnd, message, wparam, lparam; } WireEvent;
static WireEvent events[512];
static unsigned event_head, event_tail;
static void queue(uint32_t hwnd, uint32_t msg, uint32_t wp, uint32_t lp) {
    unsigned next=(event_tail+1)%512;
    if(next==event_head) return;
    events[event_tail]=(WireEvent){hwnd,msg,wp,lp}; event_tail=next;
}
@interface WireView : NSView <NSWindowDelegate>
@property uint32_t hwnd;
@property unsigned logicalWidth, logicalHeight;
@property CGImageRef image;
@end
@implementation WireView
- (BOOL)isFlipped { return YES; }
- (BOOL)acceptsFirstResponder { return YES; }
- (void)setImage:(CGImageRef)image {
    if(image) CGImageRetain(image);
    if(_image) CGImageRelease(_image);
    _image=image; [self setNeedsDisplay:YES];
}
- (void)dealloc { if(_image) CGImageRelease(_image); }
- (void)drawRect:(NSRect)dirty {
    (void)dirty;
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
    int x=(int)(p.x*_logicalWidth/self.bounds.size.width);
    int y=(int)(p.y*_logicalHeight/self.bounds.size.height);
    return ((uint32_t)y&65535)<<16|((uint32_t)x&65535);
}
- (void)mouseDown:(NSEvent *)e {queue(_hwnd,0x201,1,[self point:e]);}
- (void)mouseUp:(NSEvent *)e {queue(_hwnd,0x202,0,[self point:e]);}
- (void)rightMouseDown:(NSEvent *)e {queue(_hwnd,0x204,2,[self point:e]);}
- (void)rightMouseUp:(NSEvent *)e {queue(_hwnd,0x205,0,[self point:e]);}
- (void)mouseMoved:(NSEvent *)e {queue(_hwnd,0x200,0,[self point:e]);}
- (void)mouseDragged:(NSEvent *)e {queue(_hwnd,0x200,1,[self point:e]);}
- (unsigned)key:(NSEvent *)e {
    NSString *s=e.charactersIgnoringModifiers.uppercaseString;
    if(!s.length) return 0;
    unichar c=[s characterAtIndex:0];
    switch(c) {case NSLeftArrowFunctionKey:return 0x25;case NSUpArrowFunctionKey:return 0x26;case NSRightArrowFunctionKey:return 0x27;case NSDownArrowFunctionKey:return 0x28;case 0x7f:return 8;default:return c<=127?c:0;}
}
- (void)keyDown:(NSEvent *)e {
    unsigned key=[self key:e]; if(key) queue(_hwnd,0x100,key,1|(e.isARepeat?(1u<<30):0));
    NSString *s=e.characters;
    for(NSUInteger i=0;i<s.length;i++) {unichar c=[s characterAtIndex:i]; if(c<0xf700) queue(_hwnd,0x102,c,1);}
}
- (void)keyUp:(NSEvent *)e {unsigned key=[self key:e];if(key) queue(_hwnd,0x101,key,0xc0000001);}
@end
static NSMutableDictionary<NSNumber *,NSWindow *> *windows;
int wire_ui_init(void) {
    if(!NSThread.isMainThread) return 0;
    @autoreleasepool {
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
        [NSApp finishLaunching];
        windows=[NSMutableDictionary new];
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
int wire_ui_frame(uint32_t hwnd,const unsigned char *rgba,unsigned width,unsigned height) {
    @autoreleasepool {
        NSWindow *w=windows[@(hwnd)];if(!w) return 0;
        CFDataRef data=CFDataCreate(kCFAllocatorDefault,rgba,(CFIndex)width*height*4);
        if(!data) return 0;
        CGDataProviderRef provider=CGDataProviderCreateWithCFData(data);
        CGColorSpaceRef color=CGColorSpaceCreateDeviceRGB();
        CGImageRef image=CGImageCreate(width,height,8,32,width*4,color,kCGImageAlphaNoneSkipLast,provider,NULL,false,kCGRenderingIntentDefault);
        if(image) ((WireView *)w.contentView).image=image;
        if(image) CGImageRelease(image);CGColorSpaceRelease(color);CGDataProviderRelease(provider);CFRelease(data);
        return image!=NULL;
    }
}
int wire_ui_poll(WireEvent *out) {
    @autoreleasepool {
        for(unsigned i=0;i<64;i++) {
            NSEvent *e=[NSApp nextEventMatchingMask:NSEventMaskAny untilDate:NSDate.distantPast inMode:NSDefaultRunLoopMode dequeue:YES];
            if(!e) break;[NSApp sendEvent:e];
        }
        CFRunLoopRunInMode(kCFRunLoopDefaultMode,0.001,true);
        [NSApp updateWindows];
        if(event_head==event_tail) return 0;
        *out=events[event_head];event_head=(event_head+1)%512;return 1;
    }
}
void wire_ui_shutdown(void) {
    @autoreleasepool {for(NSWindow *w in windows.allValues){w.delegate=nil;[w close];} windows=nil;}
}
