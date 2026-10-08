#include <mach/mach.h>
#include <mach/mach_vm.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
int main(int argc, char **argv) {
    if (argc != 2) return 1;
    size_t page = (size_t)getpagesize();
    if (!strcmp(argv[1], "mmap")) mmap(0, page, PROT_READ | PROT_EXEC, MAP_PRIVATE | MAP_ANON, -1, 0);
    else if (!strcmp(argv[1], "jit")) mmap(0, page, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON | MAP_JIT, -1, 0);
    else {
        void *p = mmap(0, page, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
        if (!strcmp(argv[1], "mprotect")) mprotect(p, page, PROT_READ | PROT_EXEC);
        else if (!strcmp(argv[1], "vm_protect")) vm_protect(mach_task_self(), (vm_address_t)p, page, 0, VM_PROT_READ | VM_PROT_EXECUTE);
        else if (!strcmp(argv[1], "mach_vm_protect")) mach_vm_protect(mach_task_self(), (mach_vm_address_t)p, page, 0, VM_PROT_READ | VM_PROT_EXECUTE);
        else return 1;
    }
    return 2; // The inserted guard must terminate first with status 86.
}
