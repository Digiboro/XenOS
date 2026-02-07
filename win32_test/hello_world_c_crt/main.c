/**
 * XenOS Win32 Test Application - Hello World
 * 
 * Минимальное тестовое приложение для проверки работы user space в XenOS.
 * Использует стандартную библиотеку C (stdio.h).
 */

#include <stdio.h>
#include <stdlib.h>

int main(int argc, char *argv[])
{
    printf("=====================================\n");
    printf("  Hello from XenOS User Space!\n");
    printf("=====================================\n");
    printf("\n");
    printf("Operating System: XenOS\n");
    printf("NT Compatibility: Windows NT 6.1\n");
    printf("\n");
    printf("This is a minimal Win32 test application.\n");
    printf("If you see this message, user mode works!\n");
    printf("\n");
    
    return EXIT_SUCCESS;
}

