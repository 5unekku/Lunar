// audio_sidecar.wasm — emscripten build of a thin SDL3 audio playback wrapper.
// pushes batched interleaved f32 PCM into an SDL3 audio stream once per
// frame; no C callback, SDL just pulls whatever's been queued via
// SDL_PutAudioStreamData.

#include "sidecar_api.h"
#include <SDL3/SDL.h>
#include <SDL3/SDL_audio.h>

static SDL_AudioStream *s_stream = NULL;

int audio_init(int freq, int channels){
    if (s_stream) return 0;

    if (!SDL_Init(SDL_INIT_AUDIO)) return 0;

    SDL_AudioSpec spec = {
        .format = SDL_AUDIO_F32,
        .channels = channels,
        .freq = freq,
    };

    s_stream = SDL_OpenAudioDeviceStream(SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, &spec, NULL, NULL);
    if (!s_stream) return 0;

    // the device begins paused, same as the native side: skipping this means
    // a stream that opens successfully but never plays.
    if (!SDL_ResumeAudioStreamDevice(s_stream)) {
        SDL_DestroyAudioStream(s_stream);
        s_stream = NULL;
        return 0;
    }

    return 1;
}

void audio_push(const float *data, int num_floats){
    if (!s_stream || num_floats <= 0) return;
    SDL_PutAudioStreamData(s_stream, data, num_floats * (int)sizeof(float));
}

unsigned int audio_queued_bytes(void){
    if (!s_stream) return 0;
    int queued = SDL_GetAudioStreamQueued(s_stream);
    return queued > 0 ? (unsigned int)queued : 0;
}

void audio_shutdown(void){
    if (!s_stream) return;
    SDL_DestroyAudioStream(s_stream);
    s_stream = NULL;
}
