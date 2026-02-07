# XenOS Win32 Test Application - Hello World (CRT-less)
# Makefile для сборки под Windows x64 без C Runtime Library
#
# Требования:
#   macOS:   brew install mingw-w64
#   Ubuntu:  sudo apt install mingw-w64
#   Arch:    sudo pacman -S mingw-w64-gcc

# Кросс-компилятор для Windows x64
CC = x86_64-w64-mingw32-gcc
OBJDUMP = x86_64-w64-mingw32-objdump

# Флаги компиляции
# -Wall -Wextra  - предупреждения
# -O2            - оптимизация
# -std=c11       - стандарт C11
# -ffreestanding - freestanding окружение (без CRT)
# -fno-builtin   - не использовать встроенные функции компилятора
CFLAGS = -Wall -Wextra -O2 -std=c11 -ffreestanding -fno-builtin

# Флаги линковки для CRT-less сборки
# -nostdlib         - не линковать стандартную библиотеку
# -nodefaultlibs    - не линковать библиотеки по умолчанию
# -e _start         - точка входа (вместо main/WinMainCRTStartup)
# -Wl,--subsystem,console - консольное приложение
LDFLAGS = -nostdlib -nodefaultlibs -e _start -Wl,--subsystem,console

# Библиотеки (указываются после объектных файлов!)
LIBS = -lkernel32

# Имя выходного файла
TARGET = hello_world_crtless.exe

# Файл списка импортов
IMPORTS = imports.txt

# Исходные файлы
SOURCES = main.c

# Объектные файлы
OBJECTS = $(SOURCES:.c=.o)

.PHONY: all clean

all: $(TARGET) $(IMPORTS)

$(TARGET): $(OBJECTS)
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ $^ $(LIBS)

# Генерация списка импортируемых функций
$(IMPORTS): $(TARGET)
	@echo "Generating imports list..."
	@echo "# Imports for $(TARGET)" > $(IMPORTS)
	@echo "# Generated: $$(date)" >> $(IMPORTS)
	@echo "" >> $(IMPORTS)
	@$(OBJDUMP) -p $(TARGET) | grep -E "(DLL Name:|vma:.*Hint|[0-9a-f]+.*<none>)" >> $(IMPORTS) 2>/dev/null || true

%.o: %.c
	$(CC) $(CFLAGS) -c -o $@ $<

clean:
	rm -f $(OBJECTS) $(TARGET) $(IMPORTS)
