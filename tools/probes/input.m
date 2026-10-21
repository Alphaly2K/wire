// Original AppKit callback fixture. Events are delivered only to a private view.
#import "../../src/platform/macos.m"
#include <assert.h>
#include <stdio.h>

@interface InputEvent : NSObject
@property unsigned short keyCode;
@property NSEventModifierFlags modifierFlags;
@property NSPoint locationInWindow;
@property NSString *characters;
@property(getter=isARepeat) BOOL repeat;
@property NSInteger buttonNumber;
@property BOOL hasPreciseScrollingDeltas;
@property CGFloat scrollingDeltaX, scrollingDeltaY;
@end
@implementation InputEvent
@end

static WireEvent take(uint32_t message,uint32_t wp,uint32_t lp) {
    assert(event_head!=event_tail);
    WireEvent e=events[event_head];event_head=(event_head+1)%512;
    if(e.hwnd!=17 || e.message!=message || e.wparam!=wp || e.lparam!=lp) {
        fprintf(stderr,"event: %x %x %x %x; expected 11 %x %x %x\n",
                e.hwnd,e.message,e.wparam,e.lparam,message,wp,lp);
        abort();
    }
    return e;
}
static void empty(void) {assert(event_head==event_tail);}

int main(void) {
    @autoreleasepool {
        WireView *view=[[WireView alloc] initWithFrame:NSMakeRect(0,0,320,240)];
        view.hwnd=17;view.logicalWidth=640;view.logicalHeight=480;
        InputEvent *input=[InputEvent new];
        NSEvent *e=(NSEvent *)input;
        input.locationInWindow=NSMakePoint(-1,227.5);
        uint32_t point=(25u<<16)|65534;
        input.modifierFlags=NSEventModifierFlagShift|NSEventModifierFlagControl;
        [view mouseDown:e];take(0x201,13,point);
        [view rightMouseDown:e];take(0x204,15,point);
        input.buttonNumber=2;[view otherMouseDown:e];take(0x207,31,point);
        input.buttonNumber=3;[view otherMouseDown:e];take(0x20b,0x1003f,point);
        input.buttonNumber=4;[view otherMouseDown:e];take(0x20b,0x2007f,point);
        [view rightMouseDragged:e];take(0x200,127,point);
        [view otherMouseDragged:e];take(0x200,127,point);
        [view mouseUp:e];take(0x202,126,point);
        [view rightMouseUp:e];take(0x205,124,point);
        input.buttonNumber=2;[view otherMouseUp:e];take(0x208,108,point);
        input.buttonNumber=3;[view otherMouseUp:e];take(0x20c,0x1004c,point);
        input.buttonNumber=4;[view otherMouseUp:e];take(0x20c,0x2000c,point);
        input.scrollingDeltaY=-2;input.scrollingDeltaX=-1;
        [view scrollWheel:e];take(0x20a,0xff10000c,point);take(0x20e,0x78000c,point);
        input.hasPreciseScrollingDeltas=YES;
        input.scrollingDeltaX=0;input.scrollingDeltaY=.4;
        [view scrollWheel:e];[view scrollWheel:e];empty();
        [view scrollWheel:e];take(0x20a,0x1000c,point);empty();

        input.modifierFlags=0;input.keyCode=27;input.characters=@"_";
        [view keyDown:e];take(0x100,0xbd,0xc0001);take(0x102,'_',0xc0001);
        input.repeat=YES;[view keyDown:e];
        take(0x100,0xbd,0x400c0001);take(0x102,'_',0x400c0001);
        input.repeat=NO;[view keyUp:e];take(0x101,0xbd,0xc00c0001);
        input.keyCode=123;input.characters=@"\uf702";
        [view keyDown:e];take(0x100,0x25,0x14b0001);empty();
        [view keyUp:e];take(0x101,0x25,0xc14b0001);
        input.keyCode=90;input.characters=@"\uf717";
        [view keyDown:e];take(0x100,0x83,0x6b0001);empty();
        [view keyUp:e];take(0x101,0x83,0xc06b0001);
        input.keyCode=48;input.characters=@"\t";
        [view keyDown:e];take(0x100,9,0xf0001);take(0x102,9,0xf0001);
        [view keyUp:e];take(0x101,9,0xc00f0001);
        input.keyCode=51;input.characters=@"\x7f";
        [view keyDown:e];take(0x100,8,0xe0001);take(0x102,8,0xe0001);
        [view keyUp:e];take(0x101,8,0xc00e0001);
        input.keyCode=76;input.characters=@"\x03";
        [view keyDown:e];take(0x100,13,0x11c0001);take(0x102,13,0x11c0001);
        [view keyUp:e];take(0x101,13,0xc11c0001);
        input.keyCode=0;input.characters=@"あ\U0001f642";
        [view keyDown:e];take(0x100,65,0x1e0001);
        take(0x102,0x3042,0x1e0001);take(0x102,0xd83d,0x1e0001);take(0x102,0xde42,0x1e0001);
        [view keyUp:e];take(0x101,65,0xc01e0001);

        input.keyCode=56;input.modifierFlags=NSEventModifierFlagShift;
        [view flagsChanged:e];take(0x100,16,0x2a0001);
        input.keyCode=60;[view flagsChanged:e];take(0x100,16,0x360001);
        input.keyCode=56;[view flagsChanged:e];take(0x101,16,0xc02a0001);
        input.keyCode=60;input.modifierFlags=0;
        [view flagsChanged:e];take(0x101,16,0xc0360001);
        input.keyCode=62;input.modifierFlags=NSEventModifierFlagControl;
        [view flagsChanged:e];take(0x100,17,0x11d0001);
        input.modifierFlags=0;[view flagsChanged:e];take(0x101,17,0xc11d0001);
        input.keyCode=61;input.modifierFlags=NSEventModifierFlagOption;
        [view flagsChanged:e];take(0x104,18,0x1380001);
        input.keyCode=0;input.characters=@"å";
        [view keyDown:e];take(0x104,65,0x201e0001);take(0x106,0xe5,0x201e0001);
        [view keyUp:e];take(0x105,65,0xe01e0001);
        input.keyCode=61;input.modifierFlags=0;
        [view flagsChanged:e];take(0x105,18,0xe1380001);
        input.keyCode=57;input.modifierFlags=NSEventModifierFlagCapsLock;
        [view flagsChanged:e];take(0x100,20,0x3a0001);take(0x101,20,0xc03a0001);
        input.modifierFlags=0;[view flagsChanged:e];
        take(0x100,20,0x3a0001);take(0x101,20,0xc03a0001);

        input.keyCode=0;input.characters=@"a";
        assert([view acceptsFirstMouse:e]);
        NSNotification *activate=[NSNotification notificationWithName:NSWindowDidBecomeKeyNotification object:view];
        [view windowDidBecomeKey:activate];take(0x1c,1,0);take(6,1,0);take(7,0,0);
        [view windowDidBecomeKey:activate];empty();
        [view keyDown:e];take(0x100,65,0x1e0001);take(0x102,97,0x1e0001);
        [view mouseDown:e];take(0x201,1,point);
        NSNotification *resign=[NSNotification notificationWithName:NSWindowDidResignKeyNotification object:view];
        [view windowDidResignKey:resign];take(0x101,65,0xc01e0001);take(0x202,0,point);
        take(8,0,0);take(6,0,0);take(0x1c,0,0);
        [view windowDidResignKey:resign];empty();
        printf("native input: key/scancode/repeat, UTF-16, modifiers, five buttons, drag, wheels, focus release passed\n");
    }
    return 0;
}
