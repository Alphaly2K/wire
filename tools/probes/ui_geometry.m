#import "../../src/platform/macos.m"
#include "../../src/platform/native_graphics.h"
#include <assert.h>
#include <dlfcn.h>
#include <stdio.h>
#include <unistd.h>

static void check_size(CGSize actual,CGSize expected) {
    if(fabs(actual.width-expected.width)>.5 || fabs(actual.height-expected.height)>.5) {
        fprintf(stderr,"size %.2fx%.2f, expected %.2fx%.2f\n",actual.width,actual.height,expected.width,expected.height);
        abort();
    }
}
int main(int argc,char **argv) {
    @autoreleasepool {
        void *library=NULL,*gpu=NULL,*surface=NULL;
        char error[1024]={0};
        __typeof__(&wire_gpu_texture_create) texture_create=NULL;
        __typeof__(&wire_gpu_texture_write) texture_write=NULL;
        __typeof__(&wire_gpu_texture_free) texture_free=NULL;
        __typeof__(&wire_gpu_surface_present) present=NULL;
        __typeof__(&wire_gpu_surface_free) surface_free=NULL;
        __typeof__(&wire_gpu_close) gpu_close=NULL;
        if(argc>=3) {
            library=dlopen(argv[1],RTLD_NOW|RTLD_LOCAL);assert(library);
#define LOAD(local,name) local=(__typeof__(local))dlsym(library,#name);assert(local)
            __typeof__(&wire_gpu_open) gpu_open;
            LOAD(gpu_open,wire_gpu_open);LOAD(gpu_close,wire_gpu_close);
            LOAD(texture_create,wire_gpu_texture_create);LOAD(texture_write,wire_gpu_texture_write);
            LOAD(texture_free,wire_gpu_texture_free);LOAD(present,wire_gpu_surface_present);
            LOAD(surface_free,wire_gpu_surface_free);
            WireGpuInfo info;
            gpu=gpu_open(argv[2],&info,error,sizeof(error));
            if(!gpu) {fprintf(stderr,"%s\n",error);abort();}
        }
        assert(wire_ui_init());
        wire_ui_window(17,"Window geometry probe",314,211,1);
        void *retained=wire_ui_metal_layer(17);assert(retained);
        WireView *view=(WireView *)windows[@17].contentView;
        CAMetalLayer *layer=view.gpuLayer;
        if(gpu) {
            __typeof__(&wire_gpu_surface_create) surface_create;
            LOAD(surface_create,wire_gpu_surface_create);
            surface=surface_create(gpu,retained,error,sizeof(error));
            if(!surface) {fprintf(stderr,"%s\n",error);abort();}
        }
#undef LOAD
        check_size(layer.frame.size,view.bounds.size);
        check_size(layer.drawableSize,[view convertSizeToBacking:view.bounds.size]);
        for(unsigned i=0;i<3;i++) {
            unsigned width=i==1?480:1280,height=i==1?640:720;
            wire_ui_window(17,"Window geometry probe",width,height,1);
            void *again=wire_ui_metal_layer(17);assert(again==retained);
            wire_ui_release_metal_layer(again);
            check_size(layer.frame.size,view.bounds.size);
            check_size(layer.drawableSize,[view convertSizeToBacking:view.bounds.size]);
            assert(fabs(view.bounds.size.width/view.bounds.size.height-(double)width/height)<.002);
            NSPoint center=NSMakePoint(view.bounds.size.width/2,view.bounds.size.height/2);
            NSEvent *e=[NSEvent mouseEventWithType:NSEventTypeMouseMoved location:center modifierFlags:0 timestamp:0 windowNumber:windows[@17].windowNumber context:nil eventNumber:0 clickCount:0 pressure:0];
            uint32_t point=[view point:e];
            assert(abs((int)(point&65535)-(int)width/2)<=1);
            assert(abs((int)(point>>16)-(int)height/2)<=1);
            if(surface) {
                WireTextureDesc desc={width,height,1,1,28,0};
                void *texture=texture_create(gpu,&desc,error,sizeof(error));assert(texture);
                uint8_t *pixels=malloc((size_t)width*height*4);assert(pixels);
                for(unsigned y=0;y<height;y++) for(unsigned x=0;x<width;x++) {
                    uint8_t *p=pixels+((size_t)y*width+x)*4;
                    BOOL border=x<8 || y<8 || x+8>=width || y+8>=height;
                    p[0]=border?255:(x<width/2?220:20);
                    p[1]=border?255:(y<height/2?180:30);
                    p[2]=border?255:(x>=width/2?210:40);p[3]=255;
                }
                assert(texture_write(texture,0,pixels,(size_t)width*height*4,error,sizeof(error)));
                free(pixels);
                for(unsigned frame=0;frame<3;frame++) {
                    if(!present(surface,texture,width,height,1,error,sizeof(error))) {
                        fprintf(stderr,"%s\n",error);abort();
                    }
                    WireEvent event;wire_ui_poll(&event);
                    check_size(layer.drawableSize,[view convertSizeToBacking:view.bounds.size]);
                }
                texture_free(texture);
            }
            printf("geometry %ux%u: view %.2fx%.2f, drawable %.2fx%.2f, center %u,%u\n",width,height,view.bounds.size.width,view.bounds.size.height,layer.drawableSize.width,layer.drawableSize.height,point&65535,point>>16);
            fflush(stdout);
        }
        if(argc>=4) {
            double deadline=NSProcessInfo.processInfo.systemUptime+strtod(argv[3],NULL);
            while(NSProcessInfo.processInfo.systemUptime<deadline) {
                WireEvent event;wire_ui_poll(&event);usleep(10000);
            }
        }
        if(surface) surface_free(surface);
        if(gpu) gpu_close(gpu);
        wire_ui_release_metal_layer(retained);
        wire_ui_shutdown();
        if(library) dlclose(library);
    }
    return 0;
}
