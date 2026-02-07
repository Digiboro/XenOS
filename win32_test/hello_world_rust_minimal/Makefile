# XenOS Win32 Test Application - Hello World Minimal (Rust)
# Makefile для сборки под Windows x64
#
# Установка таргета (один раз):
#   rustup target add x86_64-pc-windows-gnu

TARGET = x86_64-pc-windows-gnu
EXE = hello_world.exe
IMPORTS = imports.txt
OBJDUMP = x86_64-w64-mingw32-objdump

.PHONY: all release debug clean

all: release

release:
	cargo build --release
	cp target/$(TARGET)/release/$(EXE) ./$(EXE)
	@echo "Generating imports list..."
	@echo "# Imports for $(EXE)" > $(IMPORTS)
	@echo "# Generated: $$(date)" >> $(IMPORTS)
	@echo "" >> $(IMPORTS)
	@$(OBJDUMP) -p $(EXE) | grep -E "(DLL Name:|vma:.*Hint|[0-9a-f]+.*<none>)" >> $(IMPORTS) 2>/dev/null || true

debug:
	cargo build
	cp target/$(TARGET)/debug/$(EXE) ./$(EXE)
	@echo "Generating imports list..."
	@echo "# Imports for $(EXE)" > $(IMPORTS)
	@echo "# Generated: $$(date)" >> $(IMPORTS)
	@echo "" >> $(IMPORTS)
	@$(OBJDUMP) -p $(EXE) | grep -E "(DLL Name:|vma:.*Hint|[0-9a-f]+.*<none>)" >> $(IMPORTS) 2>/dev/null || true

clean:
	cargo clean
	rm -f $(EXE) $(IMPORTS)
