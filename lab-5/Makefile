KERNEL := target/riscv64gc-unknown-none-elf/release/lab1-rcore
USER_ELF := user/target/riscv64gc-unknown-none-elf/release/lab5-user
QEMU ?= qemu-system-riscv64
OBJDUMP ?= rust-objdump

.PHONY: all user build qemu disasm check clean
all: build
user:
	cd user && RUSTFLAGS="-Clink-arg=-Tlinker.ld -Crelocation-model=static" cargo build --release
	test -f $(USER_ELF)
build: user
	cargo build --release
	cp $(KERNEL) kernel-qemu.elf
qemu: build
	$(QEMU) -machine virt -bios none -kernel kernel-qemu.elf -m 128M -smp 2 -nographic
disasm: build
	$(OBJDUMP) -d kernel-qemu.elf > kernel.asm
check: build
	QEMU="$(QEMU)" OBJDUMP="$(OBJDUMP)" ./scripts/check.sh kernel-qemu.elf
clean:
	cargo clean
	cd user && cargo clean
	rm -f kernel-qemu.elf kernel.asm
