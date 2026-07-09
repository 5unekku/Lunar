// batched C API exported by audio_sidecar.wasm.
// designed to cross the JS<->wasm boundary once per frame, pushing a chunk
// of interleaved f32 PCM, not once per sample.

#pragma once

// opens the default playback device at the given format. returns 0 on
// failure, nonzero on success. only one device is supported in v1.
int audio_init(int freq, int channels);

// push interleaved f32 PCM samples into the device's queue.
void audio_push(const float *data, int num_floats);

// bytes currently queued but not yet played; used to size the next push so
// the caller's jitter buffer stays near its target depth.
unsigned int audio_queued_bytes(void);

// stop playback and release the device.
void audio_shutdown(void);
