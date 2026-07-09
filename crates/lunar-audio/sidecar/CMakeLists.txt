cmake_minimum_required(VERSION 3.20)
project(audio_sidecar C)

add_executable(audio_sidecar sidecar_api.c)
set_target_properties(audio_sidecar PROPERTIES SUFFIX ".js")

target_compile_options(audio_sidecar PRIVATE "-sUSE_SDL=3" "-O3")
target_link_options(audio_sidecar PRIVATE
    "-sUSE_SDL=3"
    "-O3"
    "-sMODULARIZE=1"
    "-sEXPORT_ES6=1"
    "-sEXPORT_NAME=createAudioSidecarModule"
    "-sEXPORTED_FUNCTIONS=_audio_init,_audio_push,_audio_queued_bytes,_audio_shutdown,_malloc,_free"
    "-sEXPORTED_RUNTIME_METHODS=HEAPU8"
    "-sALLOW_MEMORY_GROWTH=1"
    "-sNO_EXIT_RUNTIME=1"
    "--no-entry"
)
