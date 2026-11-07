#!/bin/sh
set -eu

kernel=${1:-kernel-qemu.elf}
qemu=${QEMU:-qemu-system-riscv64}
objdump=${OBJDUMP:-rust-objdump}
qemu_log=$(mktemp -t lab5-qemu.XXXXXX)
asm_log=$(mktemp -t lab5-asm.XXXXXX)
qemu_pid=

cleanup() {
    if [ -n "$qemu_pid" ] && kill -0 "$qemu_pid" 2>/dev/null; then
        kill "$qemu_pid" 2>/dev/null || true
        wait "$qemu_pid" 2>/dev/null || true
    fi
    rm -f "$qemu_log" "$asm_log"
}
trap cleanup EXIT INT TERM

fail_with_log() {
    echo "$1" >&2
    echo 'QEMU output:' >&2
    sed -n '1,220p' "$qemu_log" >&2
    exit 1
}

require_output() {
    grep -F "$1" "$qemu_log" >/dev/null 2>&1 \
        || fail_with_log "missing expected output: $1"
}

"$objdump" -d "$kernel" >"$asm_log"
grep -F '<user_vector>:' "$asm_log" >/dev/null \
    || fail_with_log 'missing user_vector'
grep -F '<user_restore>:' "$asm_log" >/dev/null \
    || fail_with_log 'missing user_restore'
grep -E '[[:space:]]sret([[:space:]]|$)' "$asm_log" >/dev/null \
    || fail_with_log 'missing sret'

"$qemu" -machine virt -bios none -kernel "$kernel" -m 128M -smp 2 \
    -nographic >"$qemu_log" 2>&1 &
qemu_pid=$!

attempt=0
while [ "$attempt" -lt 150 ]; do
    if grep -F 'lab-5 syscall test: PASS' "$qemu_log" >/dev/null 2>&1; then
        break
    fi
    if ! kill -0 "$qemu_pid" 2>/dev/null; then
        fail_with_log 'QEMU exited before Lab 5 completed'
    fi
    attempt=$((attempt + 1))
    sleep 0.1
done

for output in \
    'lab-5 user-copy test: PASS' \
    'lab-5 brk test: PASS' \
    'lab-5 stack-growth test: PASS' \
    'lab-5 mmap-pool test: PASS' \
    'lab-5 mmap test: PASS' \
    'lab-5 page-table-copy test: PASS' \
    'lab-5 syscall test: PASS'; do
    require_output "$output"
done

if grep -E '(^|[[:space:]])FAIL($|[[:space:]])|invalid stack fault|unexpected trap' "$qemu_log" >/dev/null 2>&1; then
    fail_with_log 'QEMU reported a failed test or unexpected trap'
fi

sed -n '1,220p' "$qemu_log"
echo 'lab-5 check: PASS'
