// Synthesizes a seamless 6-second 1280x720 30fps H.264 starter video wallpaper
// using Windows Media Foundation IMFSinkWriter (zero external ffmpeg required).
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <mfapi.h>
#include <mfidl.h>
#include <mfreadwrite.h>
#include <mferror.h>
#include <cmath>
#include <cstdio>
#include <vector>

#pragma comment(lib, "mfplat.lib")
#pragma comment(lib, "mfreadwrite.lib")
#pragma comment(lib, "mfuuid.lib")
#pragma comment(lib, "ole32.lib")

static inline unsigned char clamp_u8(float v) {
    if (v <= 0.0f) return 0;
    if (v >= 255.0f) return 255;
    return (unsigned char)(v + 0.5f);
}

int wmain(int argc, wchar_t** argv) {
    if (argc < 2) {
        fwprintf(stderr, L"Usage: build_starter_video.exe <output.mp4>\n");
        return 1;
    }
    const wchar_t* outPath = argv[1];

    const UINT32 W = 1280;
    const UINT32 H = 720;
    const UINT32 FPS = 30;
    const UINT32 FRAMES = 180; // 6.0s exact periodic loop
    const UINT64 FRAME_DUR = 10000000ULL / FPS;
    const UINT32 BITRATE = 900000;

    HRESULT hr = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (FAILED(hr)) return 2;
    hr = MFStartup(MF_VERSION);
    if (FAILED(hr)) { CoUninitialize(); return 3; }

    IMFSinkWriter* writer = nullptr;
    IMFMediaType* outType = nullptr;
    IMFMediaType* inType = nullptr;
    DWORD streamIdx = 0;

    hr = MFCreateSinkWriterFromURL(outPath, nullptr, nullptr, &writer);
    if (FAILED(hr)) { fwprintf(stderr, L"MFCreateSinkWriterFromURL failed: 0x%08X\n", hr); goto done; }

    hr = MFCreateMediaType(&outType);
    if (FAILED(hr)) goto done;
    outType->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    outType->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_H264);
    outType->SetUINT32(MF_MT_AVG_BITRATE, BITRATE);
    outType->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
    MFSetAttributeSize(outType, MF_MT_FRAME_SIZE, W, H);
    MFSetAttributeRatio(outType, MF_MT_FRAME_RATE, FPS, 1);
    MFSetAttributeRatio(outType, MF_MT_PIXEL_ASPECT_RATIO, 1, 1);

    hr = writer->AddStream(outType, &streamIdx);
    if (FAILED(hr)) { fwprintf(stderr, L"AddStream failed: 0x%08X\n", hr); goto done; }

    hr = MFCreateMediaType(&inType);
    if (FAILED(hr)) goto done;
    inType->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    inType->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_RGB32);
    inType->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
    MFSetAttributeSize(inType, MF_MT_FRAME_SIZE, W, H);
    MFSetAttributeRatio(inType, MF_MT_FRAME_RATE, FPS, 1);
    MFSetAttributeRatio(inType, MF_MT_PIXEL_ASPECT_RATIO, 1, 1);

    hr = writer->SetInputMediaType(streamIdx, inType, nullptr);
    if (FAILED(hr)) { fwprintf(stderr, L"SetInputMediaType failed: 0x%08X\n", hr); goto done; }

    hr = writer->BeginWriting();
    if (FAILED(hr)) { fwprintf(stderr, L"BeginWriting failed: 0x%08X\n", hr); goto done; }

    {
        const float TWO_PI = 6.28318530718f;
        struct Orb { float x0, y0, rad, rx, ry, phase, cr, cg, cb, intensity; };
        const Orb orbs[8] = {
            { 0.24f, 0.36f, 0.32f, 0.045f, 0.030f, 0.0f, 248.0f, 152.0f, 54.0f, 0.58f },
            { 0.72f, 0.44f, 0.36f, 0.050f, 0.035f, 1.4f, 224.0f, 94.0f,  46.0f, 0.52f },
            { 0.48f, 0.28f, 0.28f, 0.035f, 0.025f, 2.7f, 255.0f, 198.0f, 96.0f, 0.42f },
            { 0.16f, 0.68f, 0.26f, 0.030f, 0.020f, 3.9f, 118.0f, 64.0f,  98.0f, 0.38f },
            { 0.84f, 0.26f, 0.24f, 0.040f, 0.025f, 5.1f, 255.0f, 168.0f, 76.0f, 0.44f },
            { 0.36f, 0.62f, 0.16f, 0.055f, 0.040f, 0.9f, 255.0f, 212.0f, 128.0f, 0.28f },
            { 0.62f, 0.66f, 0.19f, 0.045f, 0.030f, 2.1f, 236.0f, 124.0f, 58.0f, 0.34f },
            { 0.52f, 0.84f, 0.30f, 0.030f, 0.020f, 4.4f, 180.0f, 78.0f,  38.0f, 0.40f },
        };

        // Precompute horizontal/vertical normalized coordinates
        std::vector<float> nx(W), ny(H);
        for (UINT32 x = 0; x < W; ++x) nx[x] = (float)x / (float)(W - 1);
        for (UINT32 y = 0; y < H; ++y) ny[y] = (float)y / (float)(H - 1);

        const float aspect = (float)W / (float)H;
        const UINT32 stride = W * 4;
        const UINT32 bufSize = stride * H;

        for (UINT32 f = 0; f < FRAMES; ++f) {
            float t = (float)f / (float)FRAMES;
            float ang = TWO_PI * t;

            float ox[8], oy[8], oinvr2[8], opulse[8];
            for (int k = 0; k < 8; ++k) {
                ox[k] = orbs[k].x0 + orbs[k].rx * std::cos(ang + orbs[k].phase);
                oy[k] = orbs[k].y0 + orbs[k].ry * std::sin(ang + orbs[k].phase);
                oinvr2[k] = 1.0f / (orbs[k].rad * orbs[k].rad);
                opulse[k] = orbs[k].intensity * (0.86f + 0.14f * std::sin(ang + orbs[k].phase * 1.5f));
            }

            IMFSample* sample = nullptr;
            IMFMediaBuffer* buf = nullptr;
            hr = MFCreateMemoryBuffer(bufSize, &buf);
            if (FAILED(hr)) goto done;

            BYTE* dst = nullptr;
            hr = buf->Lock(&dst, nullptr, nullptr);
            if (FAILED(hr)) { buf->Release(); goto done; }

            // MFVideoFormat_RGB32 is bottom-up BGRX
            for (UINT32 y = 0; y < H; ++y) {
                float vy = ny[y];
                float baseR = 14.0f + vy * 34.0f;
                float baseG = 10.0f + vy * 18.0f;
                float baseB = 13.0f + (1.0f - vy) * 18.0f;

                // Subtle horizon glow band near y = 0.64
                float hd = (vy - 0.64f) * 3.8f;
                float hglow = std::exp(-hd * hd) * (0.88f + 0.12f * std::sin(ang));
                baseR += hglow * 62.0f;
                baseG += hglow * 28.0f;
                baseB += hglow * 10.0f;

                BYTE* row = dst + (H - 1 - y) * stride;
                for (UINT32 x = 0; x < W; ++x) {
                    float vx = nx[x];
                    // Silk aurora wave
                    float waveY = 0.44f + 0.08f * std::sin(vx * 5.2f + ang) + 0.04f * std::cos(vx * 9.0f - ang);
                    float wd = (vy - waveY) * 6.5f;
                    float aurora = std::exp(-wd * wd) * (0.28f + 0.14f * std::sin(vx * 3.14159f));

                    float r = baseR + aurora * 145.0f;
                    float g = baseG + aurora * 82.0f;
                    float b = baseB + aurora * 32.0f;

                    for (int k = 0; k < 8; ++k) {
                        float dx = (vx - ox[k]) * aspect;
                        float dy = (vy - oy[k]);
                        float d2 = (dx * dx + dy * dy) * oinvr2[k];
                        if (d2 < 2.2f) {
                            float w = std::exp(-d2 * 2.4f) * opulse[k];
                            r += orbs[k].cr * w;
                            g += orbs[k].cg * w;
                            b += orbs[k].cb * w;
                        }
                    }

                    // Gentle warm vignette around edges
                    float ex = (vx - 0.5f) * 1.45f;
                    float ey = (vy - 0.5f) * 1.45f;
                    float vig = 1.0f - 0.28f * (ex * ex + ey * ey);
                    if (vig < 0.58f) vig = 0.58f;

                    row[x * 4 + 0] = clamp_u8(b * vig);
                    row[x * 4 + 1] = clamp_u8(g * vig);
                    row[x * 4 + 2] = clamp_u8(r * vig);
                    row[x * 4 + 3] = 255;
                }
            }

            buf->Unlock();
            buf->SetCurrentLength(bufSize);

            hr = MFCreateSample(&sample);
            if (SUCCEEDED(hr)) {
                sample->AddBuffer(buf);
                sample->SetSampleTime((LONGLONG)f * (LONGLONG)FRAME_DUR);
                sample->SetSampleDuration((LONGLONG)FRAME_DUR);
                hr = writer->WriteSample(streamIdx, sample);
                sample->Release();
            }
            buf->Release();
            if (FAILED(hr)) { fwprintf(stderr, L"WriteSample %u failed: 0x%08X\n", f, hr); goto done; }
        }
    }

    hr = writer->Finalize();
    if (FAILED(hr)) { fwprintf(stderr, L"Finalize failed: 0x%08X\n", hr); goto done; }
    wprintf(L"STARTER_VIDEO_OK path=%s\n", outPath);

done:
    if (inType) inType->Release();
    if (outType) outType->Release();
    if (writer) writer->Release();
    MFShutdown();
    CoUninitialize();
    return SUCCEEDED(hr) ? 0 : 10;
}
