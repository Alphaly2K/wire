// Diagnose the production AudioQueue bridge without a guest or Rust harness.
// Compile from the repository root:
// clang -std=c11 -Wall -Wextra tools/probes/audio.c -framework AudioToolbox -o wire-out/audio-probe
#include <AudioToolbox/AudioToolbox.h>
#include <stdio.h>

static OSStatus report(const char *operation, OSStatus status) {
    fprintf(stderr, "%s: %d (0x%08x)\n", operation, (int)status, (unsigned)status);
    return status;
}

// The recursive token denotes the real function while this macro expands.
// Include the production implementation; do not maintain a second bridge.
#define AudioQueueNewOutput(...) report("AudioQueueNewOutput", AudioQueueNewOutput(__VA_ARGS__))
#define AudioQueueAllocateBuffer(...) report("AudioQueueAllocateBuffer", AudioQueueAllocateBuffer(__VA_ARGS__))
#define AudioQueueStart(...) report("AudioQueueStart", AudioQueueStart(__VA_ARGS__))
#include "../../src/platform/audio.c"

int main(void) {
    void *output = wire_audio_create();
    if (!output) {
        fputs("AudioQueue output could not open\n", stderr);
        return 2;
    }
    int16_t samples[2048] = {0};
    unsigned accepted = wire_audio_push(output, samples, 2048);
    uint64_t values[4] = {0};
    wire_audio_stats(output, values);
    wire_audio_destroy(output);
    printf("accepted=%u consumed=%llu queued=%llu\n", accepted,
        (unsigned long long)values[0], (unsigned long long)values[3]);
    return accepted == 2048 && values[0] + values[3] == accepted ? 0 : 1;
}
