// Public AudioQueue PCM output. Callbacks consume host samples only; guest
// memory, execution, and API calls remain on the interpreter thread.
#include <AudioToolbox/AudioToolbox.h>
#include <pthread.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
typedef struct {
    AudioQueueRef queue;
    pthread_mutex_t lock;
    int16_t samples[88200];
    unsigned head, count;
    uint64_t consumed, underrun, callbacks;
} WireAudio;
static void refill(void *ctx, AudioQueueRef queue, AudioQueueBufferRef buffer) {
    WireAudio *a=ctx;
    int16_t *out=buffer->mAudioData;
    pthread_mutex_lock(&a->lock);
    a->callbacks++;
    for(unsigned i=0;i<1024;i++) {
        if(a->count) {out[i]=a->samples[a->head];a->head=(a->head+1)%88200;a->count--;a->consumed++;}
        else {out[i]=0;a->underrun++;}
    }
    pthread_mutex_unlock(&a->lock);
    buffer->mAudioDataByteSize=2048;
    AudioQueueEnqueueBuffer(queue,buffer,0,NULL);
}
void wire_audio_destroy(void *ctx) {
    if(!ctx) return;
    WireAudio *a=ctx;
    if(a->queue) AudioQueueDispose(a->queue,true);
    pthread_mutex_destroy(&a->lock);
    free(a);
}
void *wire_audio_create(void) {
    WireAudio *a=calloc(1,sizeof(*a));
    if(!a) return NULL;
    if(pthread_mutex_init(&a->lock,NULL)) {free(a);return NULL;}
    AudioStreamBasicDescription fmt={0};
    fmt.mSampleRate=44100;
    fmt.mFormatID=kAudioFormatLinearPCM;
    fmt.mFormatFlags=kLinearPCMFormatFlagIsSignedInteger|kAudioFormatFlagIsPacked;
    fmt.mBytesPerPacket=fmt.mBytesPerFrame=4;
    fmt.mFramesPerPacket=1;fmt.mChannelsPerFrame=2;fmt.mBitsPerChannel=16;
    if(AudioQueueNewOutput(&fmt,refill,a,NULL,NULL,0,&a->queue)) {wire_audio_destroy(a);return NULL;}
    for(unsigned i=0;i<3;i++) {
        AudioQueueBufferRef b;
        if(AudioQueueAllocateBuffer(a->queue,2048,&b)) {wire_audio_destroy(a);return NULL;}
        memset(b->mAudioData,0,2048);b->mAudioDataByteSize=2048;
        if(AudioQueueEnqueueBuffer(a->queue,b,0,NULL)) {wire_audio_destroy(a);return NULL;}
    }
    if(AudioQueueStart(a->queue,NULL)) {wire_audio_destroy(a);return NULL;}
    return a;
}
unsigned wire_audio_push(void *ctx,const int16_t *samples,unsigned count) {
    WireAudio *a=ctx;
    pthread_mutex_lock(&a->lock);
    unsigned accepted=count;
    if(accepted>88200-a->count) accepted=88200-a->count;
    for(unsigned i=0;i<accepted;i++) a->samples[(a->head+a->count+i)%88200]=samples[i];
    a->count+=accepted;
    pthread_mutex_unlock(&a->lock);
    return accepted;
}
void wire_audio_stats(void *ctx,uint64_t *values) {
    WireAudio *a=ctx;
    pthread_mutex_lock(&a->lock);
    values[0]=a->consumed;values[1]=a->underrun;
    values[2]=a->callbacks;values[3]=a->count;
    pthread_mutex_unlock(&a->lock);
}
