#!/bin/sh
set -eu

kernel=${1:-kernel-qemu.elf}
qemu=${QEMU:-qemu-system-riscv64}
objdump=${OBJDUMP:-rust-objdump}
qemu_log=$(mktemp -t lab4-qemu.XXXXXX)
asm_log=$(mktemp -t lab4-asm.XXXXXX)
input_fifo=$(mktemp -u "${TMPDIR:-/tmp}/lab4-input.XXXXXX")
qemu_pid=
writer_pid=

cleanup() {
    if [ -n "$writer_pid" ] && kill -0 "$writer_pid" 2>/dev/null; then
        kill "$writer_pid" 2>/dev/null || true
    fi
    if [ -n "$qemu_pid" ] && kill -0 "$qemu_pid" 2>/dev/null; then
        kill "$qemu_pid" 2>/dev/null || true
        wait "$qemu_pid" 2>/dev/null || true
    fi
    rm -f "$qemu_log" "$asm_log" "$input_fifo"
}
trap cleanup EXIT INT TERM

fail_with_log() {
    echo "$1" >&2
    echo 'QEMU output:' >&2
    sed -n '1,200p' "$qemu_log" >&2
    exit 1
}

require_output() {
    grep -F "$1" "$qemu_log" >/dev/null 2>&1 \
        || fail_with_log "missing expected output: $1"
}

"$objdump" -d "$kernel" >"$asm_log"
for symbol in __switch user_vector user_restore; do
    grep -F "<$symbol>:" "$asm_log" >/dev/null \
        || fail_with_log "missing $symbol"
done
for instruction in sret sfence.vma; do
    grep -E "[[:space:]]$instruction([[:space:]]|$)" "$asm_log" >/dev/null \
        || fail_with_log "missing $instruction"
done

mkfifo "$input_fifo"
(
    input_attempt=0
    while [ "$input_attempt" -lt 100 ]; do
        if grep -F 'lab-4 user syscall: PASS' "$qemu_log" >/dev/null 2>&1; then
            break
        fi
        input_attempt=$((input_attempt + 1))
        sleep 0.1
    done
    printf 'K'
    sleep 2
) >"$input_fifo" &
writer_pid=$!
"$qemu" -machine virt -bios none -kernel "$kernel" -m 128M -smp 2 \
    -nographic <"$input_fifo" >"$qemu_log" 2>&1 &
qemu_pid=$!

attempt=0
while [ "$attempt" -lt 120 ]; do
    if grep -F 'lab-4 user syscall: PASS' "$qemu_log" >/dev/null 2>&1 && \
       grep -F 'lab-4 user timer: PASS' "$qemu_log" >/dev/null 2>&1 && \
       grep -F 'lab-4 user uart: PASS' "$qemu_log" >/dev/null 2>&1; then
        break
    fi
    if ! kill -0 "$qemu_pid" 2>/dev/null; then
        fail_with_log 'QEMU exited before Lab 4 completed'
    fi
    attempt=$((attempt + 1))
    sleep 0.1
done

require_output 'lab-4 address-space test: PASS'
hello_count=$(grep -F -c 'proczero: hello world!' "$qemu_log" || true)
if [ "$hello_count" -ne 2 ]; then
    fail_with_log "expected exactly two hello lines, got $hello_count"
fi
require_output 'lab-4 user syscall: PASS'
require_output 'lab-4 user timer: PASS'
require_output 'lab-4 user uart: PASS'

sed -n '1,200p' "$qemu_log"
echo 'lab-4 check: PASS'
