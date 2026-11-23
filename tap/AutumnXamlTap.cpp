#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <objbase.h>
#include <ocidl.h>
#include <oleauto.h>
#include <xamlOM.h>
#include <initguid.h>
#include <vector>
#include <string>
#include <cstdio>

// {9B4B6B71-3F5D-4B48-9B72-5C8312A0F001}
DEFINE_GUID(CLSID_AutumnXamlTap,
    0x9b4b6b71, 0x3f5d, 0x4b48, 0x9b, 0x72, 0x5c, 0x83, 0x12, 0xa0, 0xf0, 0x01);

#define WM_APP_TAP_APPLY    (WM_USER + 77)
#define WM_APP_TAP_DETACH   (WM_USER + 78)
#define WM_APP_TAP_SCHEDULE (WM_USER + 79)
#define TIMER_TAP_COALESCE  1

struct TapSharedConfig {
    DWORD version;
    DWORD active;       // 1 = apply transparent/custom fill, 0 = restore native
    DWORD tint_argb;    // 0xAARRGGBB for BackgroundFill SolidColorBrush (0x00000000 = pure clear)
    DWORD border_argb;  // reserved
};

static HMODULE g_hModule = nullptr;
static LONG g_dllRefs = 0;

static bool ReadSharedConfig(TapSharedConfig& out) {
    HANDLE hMap = OpenFileMappingW(FILE_MAP_READ, FALSE, L"Local\\AutumnTaskbarTapCfg");
    if (!hMap) return false;
    auto* ptr = (TapSharedConfig*)MapViewOfFile(hMap, FILE_MAP_READ, 0, 0, sizeof(TapSharedConfig));
    if (ptr) {
        out = *ptr;
        UnmapViewOfFile(ptr);
        CloseHandle(hMap);
        return true;
    }
    CloseHandle(hMap);
    return false;
}

static void WriteDiagLog(const wchar_t* line) {
    wchar_t tmp[MAX_PATH];
    DWORD len = GetTempPathW(MAX_PATH, tmp);
    if (len == 0 || len >= MAX_PATH - 32) return;
    wcscat_s(tmp, L"autumn_tap_diag.log");
    HANDLE h = CreateFileW(tmp, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h != INVALID_HANDLE_VALUE) {
        char buf[1024];
        int n = WideCharToMultiByte(CP_UTF8, 0, line, -1, buf, sizeof(buf) - 3, nullptr, nullptr);
        if (n > 1) {
            buf[n - 1] = '\r';
            buf[n] = '\n';
            DWORD wr = 0;
            WriteFile(h, buf, (DWORD)(n + 1), &wr, nullptr);
        }
        CloseHandle(h);
    }
}

static void FormatColorHex(DWORD argb, wchar_t* out, size_t cch) {
    swprintf_s(out, cch, L"#%08X", argb);
}

class AutumnTap final : public IObjectWithSite, public IVisualTreeServiceCallback2 {
    LONG m_refs = 1;
    IUnknown* m_site = nullptr;
    IVisualTreeService* m_vts = nullptr;
    HWND m_msgWnd = nullptr;
    std::vector<InstanceHandle> m_bgFills;
    std::vector<InstanceHandle> m_bgStrokes;
    std::vector<InstanceHandle> m_taskbarFrames;
    std::vector<InstanceHandle> m_opacityModified;
    std::vector<InstanceHandle> m_visibilityModified;
    std::vector<InstanceHandle> m_brushModified;
    bool m_advised = false;
    bool m_loggedProps = false;
    bool m_loggedApply = false;
    bool m_everTouchedBrush = false;
    bool m_applying = false;
    TapSharedConfig m_lastCfg = { 1, 1, 0x00000000, 0 };

public:
    AutumnTap() { InterlockedIncrement(&g_dllRefs); }
    ~AutumnTap() {
        Cleanup();
        InterlockedDecrement(&g_dllRefs);
    }

    // IUnknown
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** ppv) override {
        if (!ppv) return E_POINTER;
        if (riid == IID_IUnknown || riid == IID_IObjectWithSite) {
            *ppv = static_cast<IObjectWithSite*>(this);
        } else if (riid == __uuidof(IVisualTreeServiceCallback) || riid == __uuidof(IVisualTreeServiceCallback2)) {
            *ppv = static_cast<IVisualTreeServiceCallback2*>(this);
        } else {
            *ppv = nullptr;
            return E_NOINTERFACE;
        }
        AddRef();
        return S_OK;
    }

    ULONG STDMETHODCALLTYPE AddRef() override { return InterlockedIncrement(&m_refs); }
    ULONG STDMETHODCALLTYPE Release() override {
        LONG r = InterlockedDecrement(&m_refs);
        if (r == 0) delete this;
        return r;
    }

    // IObjectWithSite
    HRESULT STDMETHODCALLTYPE SetSite(IUnknown* pUnkSite) override {
        if (m_site) {
            m_site->Release();
            m_site = nullptr;
        }
        if (m_vts) {
            if (m_advised) {
                m_vts->UnadviseVisualTreeChange(this);
                m_advised = false;
            }
            m_vts->Release();
            m_vts = nullptr;
        }
        if (!pUnkSite) {
            Cleanup();
            return S_OK;
        }
        m_site = pUnkSite;
        m_site->AddRef();

        HRESULT hr = pUnkSite->QueryInterface(__uuidof(IVisualTreeService), (void**)&m_vts);
        if (SUCCEEDED(hr) && m_vts) {
            WriteDiagLog(L"SetSite: Acquired IVisualTreeService");
            hr = m_vts->AdviseVisualTreeChange(this);
            if (SUCCEEDED(hr)) {
                m_advised = true;
                WriteDiagLog(L"SetSite: AdviseVisualTreeChange OK");
            } else {
                wchar_t msg[128];
                swprintf_s(msg, L"SetSite: AdviseVisualTreeChange FAILED 0x%08X", (unsigned)hr);
                WriteDiagLog(msg);
            }
        } else {
            wchar_t msg[128];
            swprintf_s(msg, L"SetSite: QI IVisualTreeService FAILED 0x%08X", (unsigned)hr);
            WriteDiagLog(msg);
        }
        return S_OK;
    }

    HRESULT STDMETHODCALLTYPE GetSite(REFIID riid, void** ppvSite) override {
        if (!ppvSite) return E_POINTER;
        *ppvSite = nullptr;
        return m_site ? m_site->QueryInterface(riid, ppvSite) : E_FAIL;
    }

    // IVisualTreeServiceCallback
    HRESULT STDMETHODCALLTYPE OnVisualTreeChange(
        ParentChildRelation relation,
        VisualElement element,
        VisualMutationType mutationType) override
    {
        if (mutationType == Add) {
            if (element.Name) {
                if (wcscmp(element.Name, L"BackgroundFill") == 0) {
                    EnsureMsgWindow();
                    AddUnique(m_bgFills, element.Handle);
                    if (!m_loggedProps) {
                        LogProps(L"BackgroundFill", element.Handle);
                        m_loggedProps = true;
                    }
                    ApplyToFill(element.Handle);
                } else if (wcscmp(element.Name, L"BackgroundStroke") == 0) {
                    EnsureMsgWindow();
                    AddUnique(m_bgStrokes, element.Handle);
                    ApplyToStroke(element.Handle);
                } else if (wcscmp(element.Name, L"TaskbarFrame") == 0) {
                    EnsureMsgWindow();
                    AddUnique(m_taskbarFrames, element.Handle);
                    wchar_t line[256];
                    swprintf_s(line, L"Found TaskbarFrame handle=%llu type=%s",
                        (unsigned long long)element.Handle,
                        element.Type ? element.Type : L"");
                    WriteDiagLog(line);
                }
            }
        } else if (mutationType == Remove) {
            RemoveHandle(m_bgFills, element.Handle);
            RemoveHandle(m_bgStrokes, element.Handle);
            RemoveHandle(m_taskbarFrames, element.Handle);
            RemoveHandle(m_opacityModified, element.Handle);
            RemoveHandle(m_visibilityModified, element.Handle);
            RemoveHandle(m_brushModified, element.Handle);
        }
        (void)relation;
        return S_OK;
    }

    // IVisualTreeServiceCallback2
    HRESULT STDMETHODCALLTYPE OnElementStateChanged(
        InstanceHandle element,
        VisualElementState elementState,
        LPCWSTR context) override
    {
        (void)element;
        (void)elementState;
        (void)context;
        if (!m_applying && m_msgWnd) {
            PostMessageW(m_msgWnd, WM_APP_TAP_SCHEDULE, 0, 0);
        }
        return S_OK;
    }

private:
    static void AddUnique(std::vector<InstanceHandle>& vec, InstanceHandle h) {
        for (auto x : vec) if (x == h) return;
        vec.push_back(h);
    }

    static bool ContainsHandle(const std::vector<InstanceHandle>& vec, InstanceHandle h) {
        for (auto x : vec) if (x == h) return true;
        return false;
    }

    static bool RemoveHandleBool(std::vector<InstanceHandle>& vec, InstanceHandle h) {
        for (size_t i = 0; i < vec.size(); ++i) {
            if (vec[i] == h) {
                vec[i] = vec.back();
                vec.pop_back();
                return true;
            }
        }
        return false;
    }

    static void RemoveHandle(std::vector<InstanceHandle>& vec, InstanceHandle h) {
        RemoveHandleBool(vec, h);
    }

    void EnsureMsgWindow() {
        if (m_msgWnd) return;
        WNDCLASSEXW wc = {};
        wc.cbSize = sizeof(wc);
        wc.lpfnWndProc = MsgWndProc;
        wc.hInstance = g_hModule;
        wc.lpszClassName = L"AutumnTapMsgWnd";
        RegisterClassExW(&wc);
        m_msgWnd = CreateWindowExW(
            0, L"AutumnTapMsgWnd", L"AutumnTapCtrl",
            0, 0, 0, 0, 0, HWND_MESSAGE, nullptr, g_hModule, this
        );
    }

    void Cleanup() {
        if (m_msgWnd) {
            KillTimer(m_msgWnd, TIMER_TAP_COALESCE);
            DestroyWindow(m_msgWnd);
            m_msgWnd = nullptr;
        }
        m_bgFills.clear();
        m_bgStrokes.clear();
        m_taskbarFrames.clear();
        m_opacityModified.clear();
        m_visibilityModified.clear();
        m_brushModified.clear();
        if (m_vts) {
            if (m_advised) {
                m_vts->UnadviseVisualTreeChange(this);
                m_advised = false;
            }
            m_vts->Release();
            m_vts = nullptr;
        }
        if (m_site) {
            m_site->Release();
            m_site = nullptr;
        }
    }

    static LRESULT CALLBACK MsgWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
        auto* self = (AutumnTap*)GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        if (msg == WM_NCCREATE) {
            auto* cs = (CREATESTRUCTW*)lp;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (LONG_PTR)cs->lpCreateParams);
            return TRUE;
        }
        if (self) {
            if (msg == WM_APP_TAP_APPLY) {
                KillTimer(hwnd, TIMER_TAP_COALESCE);
                self->ApplyAll(wp, lp);
                return 0;
            }
            if (msg == WM_APP_TAP_SCHEDULE) {
                SetTimer(hwnd, TIMER_TAP_COALESCE, 80, nullptr);
                return 0;
            }
            if (msg == WM_TIMER && wp == TIMER_TAP_COALESCE) {
                KillTimer(hwnd, TIMER_TAP_COALESCE);
                self->ApplyAllIfActive();
                return 0;
            }
            if (msg == WM_APP_TAP_DETACH) {
                KillTimer(hwnd, TIMER_TAP_COALESCE);
                TapSharedConfig restore = { 1, 0, 0, 0 };
                self->m_lastCfg = restore;
                self->ApplyAllWith(restore);
                return self->m_everTouchedBrush ? 2 : 0;
            }
        }
        return DefWindowProcW(hwnd, msg, wp, lp);
    }

    int FindPropertyIndex(InstanceHandle handle, const wchar_t* propName) {
        if (!m_vts) return -1;
        unsigned int srcCount = 0, propCount = 0;
        PropertyChainSource* sources = nullptr;
        PropertyChainValue* props = nullptr;
        if (FAILED(m_vts->GetPropertyValuesChain(handle, &srcCount, &sources, &propCount, &props))) {
            return -1;
        }
        int found = -1;
        for (unsigned int i = 0; i < propCount; ++i) {
            if (found < 0 && props[i].PropertyName && wcscmp(props[i].PropertyName, propName) == 0) {
                found = (int)props[i].Index;
            }
            SysFreeString(props[i].Type);
            SysFreeString(props[i].DeclaringType);
            SysFreeString(props[i].ValueType);
            SysFreeString(props[i].ItemType);
            SysFreeString(props[i].Value);
            SysFreeString(props[i].PropertyName);
        }
        for (unsigned int i = 0; i < srcCount; ++i) {
            SysFreeString(sources[i].TargetType);
            SysFreeString(sources[i].Name);
            SysFreeString(sources[i].SrcInfo.FileName);
            SysFreeString(sources[i].SrcInfo.Hash);
        }
        if (sources) CoTaskMemFree(sources);
        if (props) CoTaskMemFree(props);
        return found;
    }

    void LogProps(const wchar_t* tag, InstanceHandle handle) {
        if (!m_vts) return;
        unsigned int srcCount = 0, propCount = 0;
        PropertyChainSource* sources = nullptr;
        PropertyChainValue* props = nullptr;
        if (FAILED(m_vts->GetPropertyValuesChain(handle, &srcCount, &sources, &propCount, &props))) {
            return;
        }
        for (unsigned int i = 0; i < propCount; ++i) {
            if (props[i].PropertyName &&
                (wcscmp(props[i].PropertyName, L"Fill") == 0 ||
                 wcscmp(props[i].PropertyName, L"Stroke") == 0 ||
                 wcscmp(props[i].PropertyName, L"Opacity") == 0 ||
                 wcscmp(props[i].PropertyName, L"Background") == 0 ||
                 wcscmp(props[i].PropertyName, L"Visibility") == 0))
            {
                wchar_t line[512];
                swprintf_s(line, L"  %s prop=%s idx=%u vtype=%s val=%s",
                    tag,
                    props[i].PropertyName,
                    props[i].Index,
                    props[i].ValueType ? props[i].ValueType : L"",
                    props[i].Value ? props[i].Value : L"");
                WriteDiagLog(line);
            }
            SysFreeString(props[i].Type);
            SysFreeString(props[i].DeclaringType);
            SysFreeString(props[i].ValueType);
            SysFreeString(props[i].ItemType);
            SysFreeString(props[i].Value);
            SysFreeString(props[i].PropertyName);
        }
        for (unsigned int i = 0; i < srcCount; ++i) {
            SysFreeString(sources[i].TargetType);
            SysFreeString(sources[i].Name);
            SysFreeString(sources[i].SrcInfo.FileName);
            SysFreeString(sources[i].SrcInfo.Hash);
        }
        if (sources) CoTaskMemFree(sources);
        if (props) CoTaskMemFree(props);
    }

    void ApplyAll(WPARAM wp = 0, LPARAM lp = 0) {
        if (wp == 1) {
            m_lastCfg = { 1, 1, (DWORD)lp, 0 };
        } else if (wp == 2) {
            m_lastCfg = { 1, 0, (DWORD)lp, 0 };
        } else {
            TapSharedConfig cfg = m_lastCfg;
            if (ReadSharedConfig(cfg)) {
                m_lastCfg = cfg;
            }
        }
        ApplyAllWith(m_lastCfg);
    }

    void ApplyAllIfActive() {
        TapSharedConfig cfg = m_lastCfg;
        if (ReadSharedConfig(cfg)) {
            m_lastCfg = cfg;
        }
        if (m_lastCfg.active) {
            ApplyAllWith(m_lastCfg);
        } else if (!m_opacityModified.empty() || !m_visibilityModified.empty() || !m_brushModified.empty()) {
            TapSharedConfig restore = { 1, 0, 0, 0 };
            ApplyAllWith(restore);
        }
    }

    void ApplyAllWith(const TapSharedConfig& cfg) {
        m_applying = true;
        for (auto h : m_bgFills) ApplyToFillWith(h, cfg);
        for (auto h : m_bgStrokes) ApplyToStrokeWith(h, cfg);
        m_applying = false;
    }

    void ApplyToFill(InstanceHandle h) {
        TapSharedConfig cfg = m_lastCfg;
        if (ReadSharedConfig(cfg)) {
            m_lastCfg = cfg;
        }
        ApplyToFillWith(h, m_lastCfg);
    }

    void ApplyToStroke(InstanceHandle h) {
        TapSharedConfig cfg = m_lastCfg;
        if (ReadSharedConfig(cfg)) {
            m_lastCfg = cfg;
        }
        ApplyToStrokeWith(h, m_lastCfg);
    }

    bool SetPropByName(InstanceHandle h, const wchar_t* propName, const wchar_t* typeName, const wchar_t* valStr, HRESULT* outHrCreate = nullptr, HRESULT* outHrSet = nullptr) {
        if (!m_vts) return false;
        int idx = FindPropertyIndex(h, propName);
        if (idx < 0) return false;
        BSTR bType = SysAllocString(typeName);
        BSTR bVal = SysAllocString(valStr);
        InstanceHandle inst = 0;
        HRESULT hrC = m_vts->CreateInstance(bType, bVal, &inst);
        HRESULT hrS = E_FAIL;
        bool ok = false;
        if (SUCCEEDED(hrC) && inst) {
            hrS = m_vts->SetProperty(h, inst, (unsigned int)idx);
            ok = SUCCEEDED(hrS);
        }
        SysFreeString(bType);
        SysFreeString(bVal);
        if (outHrCreate) *outHrCreate = hrC;
        if (outHrSet) *outHrSet = hrS;
        return ok;
    }

    bool HideElementNonDestructive(InstanceHandle h) {
        HRESULT hrOpC = E_FAIL, hrOpS = E_FAIL;
        if (SetPropByName(h, L"Opacity", L"Windows.Foundation.Double", L"0", &hrOpC, &hrOpS)) {
            AddUnique(m_opacityModified, h);
            if (!m_loggedApply) {
                m_loggedApply = true;
                WriteDiagLog(L"HideElementNonDestructive: Opacity=0 OK");
            }
            return true;
        }
        HRESULT hrVisC = E_FAIL, hrVisS = E_FAIL;
        if (SetPropByName(h, L"Visibility", L"Windows.UI.Xaml.Visibility", L"Collapsed", &hrVisC, &hrVisS) ||
            SetPropByName(h, L"Visibility", L"Windows.UI.Xaml.Visibility", L"1", &hrVisC, &hrVisS)) {
            AddUnique(m_visibilityModified, h);
            if (!m_loggedApply) {
                m_loggedApply = true;
                WriteDiagLog(L"HideElementNonDestructive: Visibility=Collapsed OK");
            }
            return true;
        }
        if (!m_loggedApply) {
            m_loggedApply = true;
            wchar_t msg[256];
            swprintf_s(msg, L"HideElementNonDestructive FAILED opC=0x%08X opS=0x%08X visC=0x%08X visS=0x%08X",
                (unsigned)hrOpC, (unsigned)hrOpS, (unsigned)hrVisC, (unsigned)hrVisS);
            WriteDiagLog(msg);
        }
        return false;
    }

    bool IsElementModified(InstanceHandle h) const {
        return ContainsHandle(m_opacityModified, h)
            || ContainsHandle(m_visibilityModified, h)
            || ContainsHandle(m_brushModified, h);
    }

    void RestoreElementProps(InstanceHandle h, const wchar_t* brushPropPrimary, const wchar_t* brushPropFallback) {
        if (!m_vts) return;
        if (RemoveHandleBool(m_opacityModified, h)) {
            int opIdx = FindPropertyIndex(h, L"Opacity");
            if (opIdx >= 0) {
                SetPropByName(h, L"Opacity", L"Windows.Foundation.Double", L"1");
                m_vts->ClearProperty(h, (unsigned int)opIdx);
            }
        }
        if (RemoveHandleBool(m_visibilityModified, h)) {
            int visIdx = FindPropertyIndex(h, L"Visibility");
            if (visIdx >= 0) {
                if (!SetPropByName(h, L"Visibility", L"Windows.UI.Xaml.Visibility", L"Visible")) {
                    SetPropByName(h, L"Visibility", L"Windows.UI.Xaml.Visibility", L"0");
                }
                m_vts->ClearProperty(h, (unsigned int)visIdx);
            }
        }
        if (RemoveHandleBool(m_brushModified, h)) {
            int bIdx = FindPropertyIndex(h, brushPropPrimary);
            if (bIdx < 0) bIdx = FindPropertyIndex(h, brushPropFallback);
            if (bIdx >= 0) {
                m_vts->ClearProperty(h, (unsigned int)bIdx);
            }
        }
    }

    void ApplyToFillWith(InstanceHandle h, const TapSharedConfig& cfg) {
        if (!m_vts) return;
        if (!cfg.active) {
            if (IsElementModified(h)) {
                RestoreElementProps(h, L"Fill", L"Background");
            }
            return;
        }
        // Pure transparent clear: hide non-destructively so Explorer's native AcrylicBrush is never overwritten.
        if ((cfg.tint_argb >> 24) == 0 && HideElementNonDestructive(h)) {
            return;
        }
        if (RemoveHandleBool(m_opacityModified, h)) {
            SetPropByName(h, L"Opacity", L"Windows.Foundation.Double", L"1");
        }
        const wchar_t* propName = L"Fill";
        int fillIdx = FindPropertyIndex(h, L"Fill");
        if (fillIdx < 0) {
            fillIdx = FindPropertyIndex(h, L"Background");
            propName = L"Background";
        }
        if (fillIdx < 0) return;
        wchar_t hex[16];
        FormatColorHex(cfg.tint_argb, hex, 16);
        if (SetPropByName(h, propName, L"Windows.UI.Xaml.Media.SolidColorBrush", hex)) {
            AddUnique(m_brushModified, h);
            m_everTouchedBrush = true;
        }
    }

    void ApplyToStrokeWith(InstanceHandle h, const TapSharedConfig& cfg) {
        if (!m_vts) return;
        if (!cfg.active) {
            if (IsElementModified(h)) {
                RestoreElementProps(h, L"Stroke", L"BorderBrush");
            }
            return;
        }
        if (HideElementNonDestructive(h)) {
            return;
        }
        int strokeIdx = FindPropertyIndex(h, L"Stroke");
        const wchar_t* propName = L"Stroke";
        if (strokeIdx < 0) {
            strokeIdx = FindPropertyIndex(h, L"BorderBrush");
            propName = L"BorderBrush";
        }
        if (strokeIdx < 0) return;
        if (SetPropByName(h, propName, L"Windows.UI.Xaml.Media.SolidColorBrush", L"#00000000")) {
            AddUnique(m_brushModified, h);
            m_everTouchedBrush = true;
        }
    }
};

class AutumnTapFactory final : public IClassFactory {
    LONG m_refs = 1;
public:
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** ppv) override {
        if (!ppv) return E_POINTER;
        if (riid == IID_IUnknown || riid == IID_IClassFactory) {
            *ppv = static_cast<IClassFactory*>(this);
            AddRef();
            return S_OK;
        }
        *ppv = nullptr;
        return E_NOINTERFACE;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return InterlockedIncrement(&m_refs); }
    ULONG STDMETHODCALLTYPE Release() override {
        LONG r = InterlockedDecrement(&m_refs);
        if (r == 0) delete this;
        return r;
    }
    HRESULT STDMETHODCALLTYPE CreateInstance(IUnknown* pOuter, REFIID riid, void** ppv) override {
        if (pOuter) return CLASS_E_NOAGGREGATION;
        auto* obj = new (std::nothrow) AutumnTap();
        if (!obj) return E_OUTOFMEMORY;
        HRESULT hr = obj->QueryInterface(riid, ppv);
        obj->Release();
        return hr;
    }
    HRESULT STDMETHODCALLTYPE LockServer(BOOL fLock) override {
        if (fLock) InterlockedIncrement(&g_dllRefs);
        else InterlockedDecrement(&g_dllRefs);
        return S_OK;
    }
};

extern "C" BOOL WINAPI DllMain(HMODULE hMod, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) {
        g_hModule = hMod;
        DisableThreadLibraryCalls(hMod);
    }
    return TRUE;
}

extern "C" HRESULT __stdcall DllGetClassObject(REFCLSID rclsid, REFIID riid, void** ppv) {
    if (!ppv) return E_POINTER;
    if (rclsid != CLSID_AutumnXamlTap) {
        *ppv = nullptr;
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    auto* f = new (std::nothrow) AutumnTapFactory();
    if (!f) return E_OUTOFMEMORY;
    HRESULT hr = f->QueryInterface(riid, ppv);
    f->Release();
    return hr;
}

extern "C" HRESULT __stdcall DllCanUnloadNow() {
    return g_dllRefs == 0 ? S_OK : S_FALSE;
}
