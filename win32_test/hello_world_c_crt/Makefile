# XenOS Win32 Test Application - Hello World
# Makefile для сборки под Windows x64 (mingw-w64)
#
# Требования:
#   macOS:   brew install mingw-w64
#   Ubuntu:  sudo apt install mingw-w64
#   Arch:    sudo pacman -S mingw-w64-gcc

# Кросс-компилятор для Windows x64
CC = x86_64-w64-mingw32-gcc
OBJDUMP = x86_64-w64-mingw32-objdump

# Флаги компиляции
CFLAGS = -Wall -Wextra -O2 -std=c11

# Флаги линковки - статическая линковка для независимости от runtime DLL
LDFLAGS = -static

# Имя выходного файла
TARGET = hello_world_crt.exe

# Файл списка импортов
IMPORTS = imports.txt

# Исходные файлы
SOURCES = main.c

# Объектные файлы
OBJECTS = $(SOURCES:.c=.o)

.PHONY: all clean

all: $(TARGET) $(IMPORTS)

$(TARGET): $(OBJECTS)
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ $^

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

