#ifndef AUDIOCURVE_DSP_H
#define AUDIOCURVE_DSP_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct { double gain_soft_db, gain_loud_db, compression_ratio; } ac_band_fitting;

typedef struct {
    ac_band_fitting left[12], right[12];            // speech fitting
    ac_band_fitting music_left[12], music_right[12]; // music fitting
    double mpo_dbfs;        // -20..-1
    double output_gain_db;  // -48..+12, applied before the limiter
    double speech_weight; // auto_mode = 0: 1 = Speech (speech fitting, 5/50 ms), 0 = Music (music fitting, 10/200 ms); in Auto the classifier replaces it
    double limiter_enabled; // 1 = band budget + limiter on, 0 = off (hard clamp at 0 dBFS only)
    double input_gain_db; // before the bands and the In meter/spectrum; drives the compressors
    double auto_mode; // 1 = Auto: fitting and attack/release glide between Music and Speech by the classifier; 0 = use speech_weight
    double active; // 1 = processing on, 0 = input passes through unchanged (same latency); 50 ms crossfade
} ac_params;

typedef struct { float in_rms_db[2], in_peak_db[2], out_rms_db[2], out_peak_db[2]; float reduction_db[2]; float speech; } ac_meters; // reduction_db: safety stage (band budget + limiter), dB >= 0; speech: classifier 0 = music … 1 = speech

#define AC_SPECTRUM_POINTS 96
typedef struct { float freq_hz[96]; float in_db[2][96]; float out_db[2][96]; } ac_spectrum; // [ear][point], ear 0 = left

typedef struct ac_kernel ac_kernel;

ac_kernel* ac_kernel_new(double sample_rate, uint32_t max_frames); // NULL on invalid arguments
void ac_kernel_free(ac_kernel*);
void ac_kernel_set_params(ac_kernel*, const ac_params*); // single writer, any thread, lock-free
void ac_kernel_process(ac_kernel*, float* left, float* right, uint32_t frames); // realtime, in-place
uint32_t ac_kernel_latency_frames(const ac_kernel*);
void ac_kernel_reset(ac_kernel*);
void ac_kernel_read_meters(const ac_kernel*, ac_meters* out); // any thread, reads atomics
void ac_kernel_read_spectrum(const ac_kernel*, ac_spectrum* out); // UI thread; not realtime; null-safe

#ifdef __cplusplus
}
#endif

#endif
