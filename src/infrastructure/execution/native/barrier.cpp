#include "barrier.h"
#include <usvfs/usvfs.h>
#include <new>

#ifdef MODS_BARRIER_UNIT_TEST
#include <cassert>
static unsigned terminated = 0;
static unsigned closed = 0;
static unsigned disconnected = 0;
static unsigned freed = 0;
static bool cleanup_fails = false;
static bool wait_times_out = false;
static DWORD waited_for = 0;
static BOOL test_terminate(HANDLE, UINT) { ++terminated; if (cleanup_fails) { SetLastError(ERROR_ACCESS_DENIED); return FALSE; } return TRUE; }
static DWORD test_wait(HANDLE, DWORD timeout) { waited_for = timeout; return (cleanup_fails || wait_times_out) ? WAIT_TIMEOUT : WAIT_OBJECT_0; }
static BOOL test_close(HANDLE) { ++closed; return TRUE; }
#define TerminateProcess test_terminate
#define WaitForSingleObject test_wait
#define CloseHandle test_close
#endif


// decltype preserves upstream's mixed cdecl/WINAPI conventions on x86.
struct ModsUsvfs {
  HMODULE library{};
  usvfsParameters* parameters{};
  bool connected{};
  decltype(&usvfsCreateParameters) create_parameters{};
  decltype(&usvfsFreeParameters) free_parameters{};
  decltype(&usvfsSetInstanceName) set_instance{};
  decltype(&usvfsCreateVFS) create_vfs{};
  decltype(&usvfsDisconnectVFS) disconnect{};
  decltype(&usvfsClearExecutableBlacklist) clear_blacklist{};
  decltype(&usvfsClearSkipFileSuffixes) clear_suffixes{};
  decltype(&usvfsClearSkipDirectories) clear_directories{};
  decltype(&usvfsVirtualLinkFile) link_file{};
  decltype(&usvfsVirtualLinkDirectoryStatic) link_directory{};
  decltype(&usvfsCreateProcessHooked) launch{};
};

// GetLastError is read before logging, cleanup, or any other native call.
static ModsResult native_failure() noexcept { return {1, GetLastError(), 0, 0}; }

template<class Call>
static ModsResult guarded(Call call) noexcept {
  try {
    return call();
  } catch (const std::bad_alloc&) {
    return {3, 0, 0, 0};
  } catch (...) {
    return {2, 0, 0, 0};
  }
}

// Export names checked against the pinned x86 and x64 DLL export tables.
#ifdef _WIN64
#define WINAPI_NAME(name, bytes) #name
#else
#define WINAPI_NAME(name, bytes) "_" #name "@" #bytes
#endif
#define LOAD(member, name)   session->member = reinterpret_cast<decltype(session->member)>(GetProcAddress(session->library, name));   if (!session->member) return native_failure()

ModsResult mods_usvfs_open(const wchar_t* library, const char* instance, ModsUsvfs** output) noexcept {
  *output = nullptr;
  auto* session = new (std::nothrow) ModsUsvfs;
  if (!session) return {3, 0, 0, 0};

  auto result = guarded([&]() -> ModsResult {
    session->library = LoadLibraryExW(library, nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!session->library) return native_failure();
    LOAD(create_parameters, "usvfsCreateParameters");
    LOAD(free_parameters, "usvfsFreeParameters");
    LOAD(set_instance, "usvfsSetInstanceName");
    LOAD(create_vfs, WINAPI_NAME(usvfsCreateVFS, 4));
    LOAD(disconnect, WINAPI_NAME(usvfsDisconnectVFS, 0));
    LOAD(clear_blacklist, WINAPI_NAME(usvfsClearExecutableBlacklist, 0));
    LOAD(clear_suffixes, WINAPI_NAME(usvfsClearSkipFileSuffixes, 0));
    LOAD(clear_directories, WINAPI_NAME(usvfsClearSkipDirectories, 0));
    LOAD(link_file, WINAPI_NAME(usvfsVirtualLinkFile, 12));
    LOAD(link_directory, WINAPI_NAME(usvfsVirtualLinkDirectoryStatic, 12));
    LOAD(launch, WINAPI_NAME(usvfsCreateProcessHooked, 40));
    session->parameters = session->create_parameters();
    if (!session->parameters) return {3, 0, 0, 0};
    session->set_instance(session->parameters, instance);
    // A failed/throwing create may already have changed upstream global state.
    session->connected = true;
    SetLastError(ERROR_SUCCESS);
    if (!session->create_vfs(session->parameters)) return native_failure();
    return {};
  });
  if (result.status != 0) {
    auto cleanup = mods_usvfs_close(session);
    result.cleanup_status = cleanup.status;
    result.cleanup_error = cleanup.native_error;
    return result;
  }
  *output = session;
  return {};
}
#undef LOAD
#undef WINAPI_NAME

ModsResult mods_usvfs_clear_bypasses(ModsUsvfs* session) noexcept {
  return guarded([&]() -> ModsResult {
    session->clear_blacklist();
    session->clear_suffixes();
    session->clear_directories();
    return {};
  });
}

ModsResult mods_usvfs_link_file(ModsUsvfs* session, const wchar_t* source, const wchar_t* destination) noexcept {
  return guarded([&]() -> ModsResult {
    SetLastError(ERROR_SUCCESS);
    if (!session->link_file(source, destination, 0)) return native_failure();
    return {};
  });
}

ModsResult mods_usvfs_link_directory(ModsUsvfs* session, const wchar_t* source, const wchar_t* destination,
                                    unsigned int flags) noexcept {
  return guarded([&]() -> ModsResult {
    SetLastError(ERROR_SUCCESS);
    if (!session->link_directory(source, destination, flags)) return native_failure();
    return {};
  });
}

// STARTUPINFOEX is passed through unmodified upstream. Restrict inheritance to
// the three selected child streams instead of every inheritable controller handle.
struct StartupAttributes {
  LPPROC_THREAD_ATTRIBUTE_LIST list{};
  bool initialized{};
  ~StartupAttributes() {
    if (initialized) DeleteProcThreadAttributeList(list);
    if (list) HeapFree(GetProcessHeap(), 0, list);
  }
};

ModsResult mods_usvfs_launch(ModsUsvfs* session, const wchar_t* application, wchar_t* command,
                            const wchar_t* directory, STARTUPINFOW* startup, BOOL inherit_handles, BOOL new_process_group,
                            PROCESS_INFORMATION* output) noexcept {
  *output = {};
  PROCESS_INFORMATION created{};
  auto result = guarded([&]() -> ModsResult {
    StartupAttributes attributes;
    STARTUPINFOEXW extended{};
    DWORD flags = CREATE_SUSPENDED;
    if (new_process_group) flags |= CREATE_NEW_PROCESS_GROUP;
    if (inherit_handles) {
      if (!startup || !(startup->dwFlags & STARTF_USESTDHANDLES)) return {1, ERROR_INVALID_PARAMETER, 0, 0};
      SIZE_T bytes = 0;
      InitializeProcThreadAttributeList(nullptr, 1, 0, &bytes);
      if (bytes == 0) return native_failure();
      attributes.list = static_cast<LPPROC_THREAD_ATTRIBUTE_LIST>(HeapAlloc(GetProcessHeap(), 0, bytes));
      if (!attributes.list) return {3, ERROR_NOT_ENOUGH_MEMORY, 0, 0};
      if (!InitializeProcThreadAttributeList(attributes.list, 1, 0, &bytes)) return native_failure();
      attributes.initialized = true;
      HANDLE handles[] = {startup->hStdInput, startup->hStdOutput, startup->hStdError};
      if (!UpdateProcThreadAttribute(attributes.list, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                                     handles, sizeof(handles), nullptr, nullptr)) return native_failure();
      extended.StartupInfo = *startup;
      extended.StartupInfo.cb = sizeof(extended);
      extended.lpAttributeList = attributes.list;
      flags |= EXTENDED_STARTUPINFO_PRESENT;
      SetLastError(ERROR_SUCCESS);
      if (!session->launch(application, command, nullptr, nullptr, TRUE, flags,
                           nullptr, directory, &extended.StartupInfo, &created)) return native_failure();
      return {};
    }
    SetLastError(ERROR_SUCCESS);
    if (!session->launch(application, command, nullptr, nullptr, FALSE, flags,
                         nullptr, directory, startup, &created)) return native_failure();
    return {};
  });
  if (result.status == 0) {
    *output = created;
    return result;
  }

  // Upstream can throw after CreateProcessW, or return FALSE after terminating it.
  // No failed launch transfers a handle to Rust or resumes the suspended root.
  if (created.hProcess) {
    if (!TerminateProcess(created.hProcess, 125)) {
      auto error = GetLastError();
      if (WaitForSingleObject(created.hProcess, 0) != WAIT_OBJECT_0) result.cleanup_error = error;
    } else {
      const auto waited = WaitForSingleObject(created.hProcess, MODS_CLEANUP_TIMEOUT_MS);
      if (waited == WAIT_FAILED) result.cleanup_error = GetLastError();
      else if (waited != WAIT_OBJECT_0) result.cleanup_error = ERROR_TIMEOUT;
    }
  }
  if (created.hThread && !CloseHandle(created.hThread) && result.cleanup_error == 0) result.cleanup_error = GetLastError();
  if (created.hProcess && !CloseHandle(created.hProcess) && result.cleanup_error == 0) result.cleanup_error = GetLastError();
  if (result.cleanup_error != 0) result.cleanup_status = 1;
  return result;
}

ModsResult mods_usvfs_close(ModsUsvfs* session) noexcept {
  auto result = guarded([&]() -> ModsResult {
    if (session->connected) session->disconnect();
    session->connected = false;
    if (session->parameters) session->free_parameters(session->parameters);
    session->parameters = nullptr;
    return {};
  });
  // Do not unload code referenced by possibly-live upstream state after an exception.
  // Rust permanently poisons its one-session gate when close fails.
  if (result.status != 0) return result;
  if (session->library && !FreeLibrary(session->library)) result = native_failure();
  delete session;
  return result;
}

#ifdef MODS_BARRIER_UNIT_TEST
static BOOL WINAPI fail_after_create(LPCWSTR, LPWSTR, LPSECURITY_ATTRIBUTES, LPSECURITY_ATTRIBUTES,
                                     BOOL, DWORD, LPVOID, LPCWSTR, LPSTARTUPINFOW, LPPROCESS_INFORMATION output) {
  output->hProcess = reinterpret_cast<HANDLE>(1);
  output->hThread = reinterpret_cast<HANDLE>(2);
  throw 7;
}
static BOOL WINAPI native_failure_after_create(LPCWSTR, LPWSTR, LPSECURITY_ATTRIBUTES, LPSECURITY_ATTRIBUTES,
                                               BOOL, DWORD, LPVOID, LPCWSTR, LPSTARTUPINFOW, LPPROCESS_INFORMATION output) {
  output->hProcess = reinterpret_cast<HANDLE>(1);
  output->hThread = reinterpret_cast<HANDLE>(2);
  SetLastError(ERROR_ACCESS_DENIED);
  return FALSE;
}
static BOOL WINAPI successful_launch(LPCWSTR, LPWSTR, LPSECURITY_ATTRIBUTES, LPSECURITY_ATTRIBUTES,
                                     BOOL, DWORD, LPVOID, LPCWSTR, LPSTARTUPINFOW, LPPROCESS_INFORMATION output) {
  output->hProcess = reinterpret_cast<HANDLE>(1);
  output->hThread = reinterpret_cast<HANDLE>(2);
  return TRUE;
}
static void WINAPI disconnect_once() { ++disconnected; }
static void free_once(usvfsParameters*) { ++freed; }
int main() {
  assert(guarded([]() -> ModsResult { throw std::bad_alloc(); }).status == 3);
  assert(guarded([]() -> ModsResult { throw 7; }).status == 2);
  ModsUsvfs session{};
  session.launch = fail_after_create;
  PROCESS_INFORMATION output{};
  auto result = mods_usvfs_launch(&session, nullptr, nullptr, nullptr, nullptr, FALSE, FALSE, &output);
  assert(result.status == 2 && result.native_error == 0);
  assert(output.hProcess == nullptr && output.hThread == nullptr);
  assert(terminated == 1 && closed == 2 && waited_for == MODS_CLEANUP_TIMEOUT_MS);
  session.launch = native_failure_after_create;
  result = mods_usvfs_launch(&session, nullptr, nullptr, nullptr, nullptr, FALSE, FALSE, &output);
  assert(result.status == 1 && result.native_error == ERROR_ACCESS_DENIED);
  assert(terminated == 2 && closed == 4);
  session.launch = successful_launch;
  result = mods_usvfs_launch(&session, nullptr, nullptr, nullptr, nullptr, FALSE, FALSE, &output);
  assert(result.status == 0 && output.hProcess == reinterpret_cast<HANDLE>(1));
  assert(output.hThread == reinterpret_cast<HANDLE>(2));
  assert(terminated == 2 && closed == 4);
  cleanup_fails = true;
  session.launch = fail_after_create;
  result = mods_usvfs_launch(&session, nullptr, nullptr, nullptr, nullptr, FALSE, FALSE, &output);
  assert(result.status == 2 && result.cleanup_status == 1 && result.cleanup_error == ERROR_ACCESS_DENIED);
  assert(output.hProcess == nullptr && output.hThread == nullptr && terminated == 3 && closed == 6);
  cleanup_fails = false;
  wait_times_out = true;
  result = mods_usvfs_launch(&session, nullptr, nullptr, nullptr, nullptr, FALSE, FALSE, &output);
  assert(result.status == 2 && result.cleanup_status == 1 && result.cleanup_error == ERROR_TIMEOUT);
  assert(waited_for == MODS_CLEANUP_TIMEOUT_MS && terminated == 4 && closed == 8);
  auto* owned = new ModsUsvfs;
  owned->connected = true;
  owned->disconnect = disconnect_once;
  owned->parameters = reinterpret_cast<usvfsParameters*>(1);
  owned->free_parameters = free_once;
  result = mods_usvfs_close(owned);
  assert(result.status == 0 && disconnected == 1 && freed == 1);
}
#endif
