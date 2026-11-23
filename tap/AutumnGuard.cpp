#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shellapi.h>
#include <objbase.h>
#include <oleauto.h>
#include <taskschd.h>
#include <cstdio>
#include <cstdlib>
#include <cwchar>

struct AutoBstr {
    BSTR b = nullptr;
    AutoBstr() = default;
    explicit AutoBstr(const wchar_t* s) { b = s ? SysAllocString(s) : nullptr; }
    ~AutoBstr() { if (b) SysFreeString(b); }
    AutoBstr(const AutoBstr&) = delete;
    AutoBstr& operator=(const AutoBstr&) = delete;
    operator BSTR() const { return b; }
};

static bool BuildSiblingPath(const wchar_t* fileName, wchar_t* outPath, size_t cchPath, wchar_t* outDir, size_t cchDir) {
    DWORD n = GetModuleFileNameW(nullptr, outPath, (DWORD)cchPath);
    if (n == 0 || n >= cchPath) return false;
    wchar_t* slash = wcsrchr(outPath, L'\\');
    if (!slash) return false;
    size_t dirLen = (size_t)(slash - outPath);
    if (dirLen + 1 >= cchDir) return false;
    wcsncpy_s(outDir, cchDir, outPath, dirLen);
    outDir[dirLen] = L'\0';
    wcscpy_s(slash + 1, cchPath - (dirLen + 1), fileName);
    return true;
}

static void SpawnFallbackWatch(const wchar_t* guardExe, DWORD ppid) {
    wchar_t cmd[MAX_PATH + 64];
    swprintf_s(cmd, L"\"%s\" --watch %lu", guardExe, (unsigned long)ppid);
    STARTUPINFOW si = {};
    si.cb = sizeof(si);
    PROCESS_INFORMATION pi = {};
    const DWORD flagsBreakaway = CREATE_BREAKAWAY_FROM_JOB | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
    const DWORD flagsPlain = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
    if (!CreateProcessW(nullptr, cmd, nullptr, nullptr, FALSE, flagsBreakaway, nullptr, nullptr, &si, &pi)) {
        if (!CreateProcessW(nullptr, cmd, nullptr, nullptr, FALSE, flagsPlain, nullptr, nullptr, &si, &pi)) {
            return;
        }
    }
    if (pi.hThread) CloseHandle(pi.hThread);
    if (pi.hProcess) CloseHandle(pi.hProcess);
}

static bool LaunchViaTaskScheduler(const wchar_t* guardExe, const wchar_t* workDir, DWORD ppid) {
    HRESULT hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    bool comInited = SUCCEEDED(hr);
    if (hr == RPC_E_CHANGED_MODE) {
        hr = S_OK;
    }
    if (FAILED(hr)) return false;

    bool launched = false;
    ITaskService* pService = nullptr;
    ITaskFolder* pFolder = nullptr;
    ITaskDefinition* pTask = nullptr;
    IRegistrationInfo* pRegInfo = nullptr;
    IPrincipal* pPrincipal = nullptr;
    ITaskSettings* pSettings = nullptr;
    IActionCollection* pActions = nullptr;
    IAction* pAction = nullptr;
    IExecAction* pExec = nullptr;
    IRegisteredTask* pRegTask = nullptr;
    IRunningTask* pRunning = nullptr;

    wchar_t taskName[64];
    swprintf_s(taskName, L"AutumnDeckGuard_%lu", (unsigned long)ppid);
    wchar_t args[64];
    swprintf_s(args, L"--watch %lu", (unsigned long)ppid);

    VARIANT vtEmpty;
    VariantInit(&vtEmpty);

    hr = CoCreateInstance(CLSID_TaskScheduler, nullptr, CLSCTX_INPROC_SERVER, IID_ITaskService, (void**)&pService);
    if (FAILED(hr) || !pService) goto cleanup;

    hr = pService->Connect(vtEmpty, vtEmpty, vtEmpty, vtEmpty);
    if (FAILED(hr)) goto cleanup;

    {
        AutoBstr bRoot(L"\\");
        hr = pService->GetFolder(bRoot, &pFolder);
        if (FAILED(hr) || !pFolder) goto cleanup;
    }

    hr = pService->NewTask(0, &pTask);
    if (FAILED(hr) || !pTask) goto cleanup;

    if (SUCCEEDED(pTask->get_RegistrationInfo(&pRegInfo)) && pRegInfo) {
        AutoBstr bAuthor(L"AutumnDeck");
        pRegInfo->put_Author(bAuthor);
    }

    if (SUCCEEDED(pTask->get_Principal(&pPrincipal)) && pPrincipal) {
        pPrincipal->put_LogonType(TASK_LOGON_INTERACTIVE_TOKEN);
        pPrincipal->put_RunLevel(TASK_RUNLEVEL_LUA);
    }

    if (SUCCEEDED(pTask->get_Settings(&pSettings)) && pSettings) {
        AutoBstr bLimit(L"PT0S");
        pSettings->put_Hidden(VARIANT_TRUE);
        pSettings->put_DisallowStartIfOnBatteries(VARIANT_FALSE);
        pSettings->put_StopIfGoingOnBatteries(VARIANT_FALSE);
        pSettings->put_AllowDemandStart(VARIANT_TRUE);
        pSettings->put_ExecutionTimeLimit(bLimit);
        pSettings->put_MultipleInstances(TASK_INSTANCES_PARALLEL);
    }

    hr = pTask->get_Actions(&pActions);
    if (FAILED(hr) || !pActions) goto cleanup;

    hr = pActions->Create(TASK_ACTION_EXEC, &pAction);
    if (FAILED(hr) || !pAction) goto cleanup;

    hr = pAction->QueryInterface(IID_IExecAction, (void**)&pExec);
    if (FAILED(hr) || !pExec) goto cleanup;

    {
        AutoBstr bPath(guardExe);
        AutoBstr bArgs(args);
        AutoBstr bDir(workDir);
        if (FAILED(pExec->put_Path(bPath))) goto cleanup;
        pExec->put_Arguments(bArgs);
        pExec->put_WorkingDirectory(bDir);
    }

    {
        AutoBstr bName(taskName);
        hr = pFolder->RegisterTaskDefinition(
            bName,
            pTask,
            TASK_CREATE_OR_UPDATE,
            vtEmpty,
            vtEmpty,
            TASK_LOGON_INTERACTIVE_TOKEN,
            vtEmpty,
            &pRegTask
        );
        if (FAILED(hr) || !pRegTask) goto cleanup;

        hr = pRegTask->Run(vtEmpty, &pRunning);
        if (SUCCEEDED(hr) && pRunning) {
            for (int i = 0; i < 15; ++i) {
                TASK_STATE st = TASK_STATE_UNKNOWN;
                if (SUCCEEDED(pRunning->get_State(&st)) && st == TASK_STATE_RUNNING) {
                    launched = true;
                    break;
                }
                Sleep(20);
            }
            if (!launched) {
                // Service accepted Run(); wait briefly before removing definition
                Sleep(60);
                launched = true;
            }
        }
        pFolder->DeleteTask(bName, 0);
    }

cleanup:
    if (pRunning) pRunning->Release();
    if (pRegTask) pRegTask->Release();
    if (pExec) pExec->Release();
    if (pAction) pAction->Release();
    if (pActions) pActions->Release();
    if (pSettings) pSettings->Release();
    if (pPrincipal) pPrincipal->Release();
    if (pRegInfo) pRegInfo->Release();
    if (pTask) pTask->Release();
    if (pFolder) pFolder->Release();
    if (pService) pService->Release();
    VariantClear(&vtEmpty);
    if (comInited) CoUninitialize();
    return launched;
}

static int RunWatch(DWORD ppid) {
    if (ppid == 0) return 0;

    wchar_t evName[96];
    swprintf_s(evName, L"Local\\AutumnDeckCleanExit_%lu", (unsigned long)ppid);
    HANDLE hClean = OpenEventW(SYNCHRONIZE, FALSE, evName);

    if (hClean && WaitForSingleObject(hClean, 0) == WAIT_OBJECT_0) {
        CloseHandle(hClean);
        return 0;
    }

    HANDLE hProc = OpenProcess(SYNCHRONIZE, FALSE, ppid);
    if (hProc) {
        WaitForSingleObject(hProc, INFINITE);
        CloseHandle(hProc);
    }

    Sleep(50);
    bool clean = (hClean != nullptr && WaitForSingleObject(hClean, 0) == WAIT_OBJECT_0);
    if (hClean) CloseHandle(hClean);
    if (clean) return 0;

    wchar_t mainExe[MAX_PATH] = {};
    wchar_t workDir[MAX_PATH] = {};
    if (!BuildSiblingPath(L"AutumnDeck.exe", mainExe, MAX_PATH, workDir, MAX_PATH)) {
        return 0;
    }

    wchar_t cmd[MAX_PATH + 32];
    swprintf_s(cmd, L"\"%s\" --restore", mainExe);
    STARTUPINFOW si = {};
    si.cb = sizeof(si);
    PROCESS_INFORMATION pi = {};
    if (CreateProcessW(nullptr, cmd, nullptr, nullptr, FALSE, CREATE_NO_WINDOW, nullptr, workDir, &si, &pi)) {
        if (pi.hThread) CloseHandle(pi.hThread);
        if (pi.hProcess) {
            WaitForSingleObject(pi.hProcess, 12000);
            CloseHandle(pi.hProcess);
        }
    }
    return 0;
}

int WINAPI wWinMain(HINSTANCE, HINSTANCE, PWSTR, int) {
    int argc = 0;
    LPWSTR* argv = CommandLineToArgvW(GetCommandLineW(), &argc);
    if (!argv || argc < 3) {
        if (argv) LocalFree(argv);
        return 0;
    }

    const wchar_t* mode = argv[1];
    DWORD ppid = (DWORD)wcstoul(argv[2], nullptr, 10);
    int rc = 0;

    if (_wcsicmp(mode, L"--arm") == 0 && ppid != 0) {
        wchar_t guardExe[MAX_PATH] = {};
        wchar_t workDir[MAX_PATH] = {};
        if (BuildSiblingPath(L"AutumnGuard.exe", guardExe, MAX_PATH, workDir, MAX_PATH)) {
            if (!LaunchViaTaskScheduler(guardExe, workDir, ppid)) {
                SpawnFallbackWatch(guardExe, ppid);
            }
        }
    } else if (_wcsicmp(mode, L"--watch") == 0 && ppid != 0) {
        rc = RunWatch(ppid);
    }

    LocalFree(argv);
    return rc;
}
