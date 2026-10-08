// Independent macOS probe guard. Insert only into owned probe processes.
// Mono also has a separate compile-time guard in mono-mmap.c.
#include <mach/mach.h>
#include <mach/mach_vm.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <unistd.h>

#define INTERPOSE(replacement, original) \
    __attribute__((used)) static const struct { const void *new_func; const void *old_func; } \
    replacement##_entry __attribute__((section("__DATA,__interpose"))) = \
    { (const void *)replacement, (const void *)original }

static _Atomic unsigned long calls;
static void check(int executable, const char *route) {
    atomic_fetch_add_explicit(&calls, 1, memory_order_relaxed);
    if (executable) {
        char message[160];
        int length = snprintf(message, sizeof(message), "WIRE_NOEXEC_GUARD denied %s\n", route);
        write(STDERR_FILENO, message, (size_t)length);
        _exit(86);
    }
}
static void *guard_mmap(void *addr, size_t size, int prot, int flags, int fd, off_t offset) {
    check((prot & PROT_EXEC) || (flags & MAP_JIT), "mmap");
    return mmap(addr, size, prot, flags, fd, offset);
}
static int guard_mprotect(void *addr, size_t size, int prot) {
    check(prot & PROT_EXEC, "mprotect");
    return mprotect(addr, size, prot);
}
static kern_return_t guard_vm_protect(vm_map_t task, vm_address_t addr, vm_size_t size,
                                      boolean_t maximum, vm_prot_t prot) {
    check(prot & VM_PROT_EXECUTE, "vm_protect");
    return vm_protect(task, addr, size, maximum, prot);
}
static kern_return_t guard_mach_vm_protect(vm_map_t task, mach_vm_address_t addr,
                                          mach_vm_size_t size, boolean_t maximum, vm_prot_t prot) {
    check(prot & VM_PROT_EXECUTE, "mach_vm_protect");
    return mach_vm_protect(task, addr, size, maximum, prot);
}
INTERPOSE(guard_mmap, mmap);
INTERPOSE(guard_mprotect, mprotect);
INTERPOSE(guard_vm_protect, vm_protect);
INTERPOSE(guard_mach_vm_protect, mach_vm_protect);

__attribute__((destructor)) static void report(void) {
    fprintf(stderr, "WIRE_NOEXEC_GUARD checks=%lu denied=0\n",
            atomic_load_explicit(&calls, memory_order_relaxed));
}
