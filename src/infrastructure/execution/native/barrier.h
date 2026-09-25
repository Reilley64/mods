// Private exception boundary. Upstream declarations remain authoritative.
#pragma once
#include <Windows.h>
#include <stdint.h>

#define MODS_CLEANUP_TIMEOUT_MS 5000

struct ModsUsvfs;
#ifndef __cplusplus
typedef struct ModsUsvfs ModsUsvfs;
#endif
typedef struct ModsResult {
  uint32_t status; // 0 success, 1 native failure, 2 C++ exception, 3 allocation failure
  uint32_t native_error;
  uint32_t cleanup_status;
  uint32_t cleanup_error;
} ModsResult;
#ifdef __cplusplus
#define MODS_NOEXCEPT noexcept
extern "C" {
#else
#define MODS_NOEXCEPT
#endif
ModsResult mods_usvfs_open(const wchar_t* library, const char* instance, ModsUsvfs** output) MODS_NOEXCEPT;
ModsResult mods_usvfs_clear_bypasses(ModsUsvfs* session) MODS_NOEXCEPT;
ModsResult mods_usvfs_link_file(ModsUsvfs* session, const wchar_t* source, const wchar_t* destination) MODS_NOEXCEPT;
ModsResult mods_usvfs_link_directory(ModsUsvfs* session, const wchar_t* source, const wchar_t* destination, unsigned int flags) MODS_NOEXCEPT;
ModsResult mods_usvfs_launch(ModsUsvfs* session, const wchar_t* application, wchar_t* command,
                            const wchar_t* directory, STARTUPINFOW* startup, BOOL inherit_handles, BOOL new_process_group,
                            PROCESS_INFORMATION* output) MODS_NOEXCEPT;
ModsResult mods_usvfs_close(ModsUsvfs* session) MODS_NOEXCEPT;
#ifdef __cplusplus
}
#endif
#undef MODS_NOEXCEPT
