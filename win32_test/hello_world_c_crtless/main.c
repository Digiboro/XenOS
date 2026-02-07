/**
 * XenOS Win32 Test Application - Hello World (CRT-less)
 * 
 * Минимальное тестовое приложение без использования C Runtime Library.
 * Напрямую вызывает Windows API функции (kernel32.dll).
 * 
 * Особенности:
 * - Не использует stdio.h, stdlib.h и другие CRT заголовки
 * - Точка входа: _start (вместо main)
 * - Вывод через WriteFile + GetStdHandle
 * - Завершение через ExitProcess
 */

// =============================================================================
// Типы данных Windows
// =============================================================================

typedef unsigned long       DWORD;
typedef unsigned long long  HANDLE;
typedef int                 BOOL;
typedef const char*         LPCSTR;
typedef char*               LPSTR;
typedef void*               LPVOID;
typedef const void*         LPCVOID;
typedef DWORD*              LPDWORD;

// Специальные значения
#define NULL ((void*)0)
#define TRUE 1
#define FALSE 0

// Стандартные дескрипторы
#define STD_INPUT_HANDLE  ((DWORD)-10)
#define STD_OUTPUT_HANDLE ((DWORD)-11)
#define STD_ERROR_HANDLE  ((DWORD)-12)

// Недействительный дескриптор
#define INVALID_HANDLE_VALUE ((HANDLE)-1)

// =============================================================================
// Импорты из kernel32.dll
// =============================================================================

__declspec(dllimport) HANDLE __stdcall GetStdHandle(DWORD nStdHandle);
__declspec(dllimport) BOOL __stdcall WriteFile(
    HANDLE hFile,
    LPCVOID lpBuffer,
    DWORD nNumberOfBytesToWrite,
    LPDWORD lpNumberOfBytesWritten,
    LPVOID lpOverlapped
);
__declspec(dllimport) void __stdcall ExitProcess(DWORD uExitCode);

// =============================================================================
// Вспомогательные функции
// =============================================================================

/**
 * Вычисляет длину строки (аналог strlen)
 */
static inline DWORD str_length(LPCSTR str) {
    DWORD len = 0;
    while (str[len] != '\0') {
        len++;
    }
    return len;
}

/**
 * Выводит строку в stdout
 */
static void print(LPCSTR str) {
    HANDLE stdout_handle = GetStdHandle(STD_OUTPUT_HANDLE);
    if (stdout_handle == INVALID_HANDLE_VALUE) {
        return;
    }
    
    DWORD len = str_length(str);
    DWORD written;
    WriteFile(stdout_handle, str, len, &written, NULL);
}

// =============================================================================
// Точка входа (без CRT)
// =============================================================================

/**
 * Точка входа приложения без CRT.
 * Вызывается напрямую загрузчиком Windows, минуя CRT инициализацию.
 */
void _start(void) {
    print("=====================================\n");
    print("  Hello from XenOS User Space!\n");
    print("  (CRT-less C application)\n");
    print("=====================================\n");
    print("\n");
    print("Operating System: XenOS\n");
    print("NT Compatibility: Windows NT 6.1\n");
    print("\n");
    print("This application does NOT use C Runtime Library.\n");
    print("Direct Windows API calls only:\n");
    print("  - GetStdHandle (kernel32.dll)\n");
    print("  - WriteFile (kernel32.dll)\n");
    print("  - ExitProcess (kernel32.dll)\n");
    print("\n");
    print("If you see this message, CRT-less execution works!\n");
    print("\n");
    
    ExitProcess(0);
}
