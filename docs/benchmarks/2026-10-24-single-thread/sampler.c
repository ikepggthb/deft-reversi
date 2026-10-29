/* Diagnostic sampler for Linux x86_64 single-thread workloads only.
 * Samples executable instruction pointers with ITIMER_PROF. Output is selected
 * by DEFT_SAMPLE_OUT. This is not linked into the engine or timed benchmarks.
 */
#define _GNU_SOURCE
#include <link.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/time.h>
#include <ucontext.h>

_Static_assert(ATOMIC_LONG_LOCK_FREE == 2 && ATOMIC_INT_LOCK_FREE == 2,
               "signal handler needs lock-free atomics");
static _Atomic uintptr_t image_base, image_low = UINTPTR_MAX, image_high;
static _Atomic uintptr_t keys[65536];
static _Atomic unsigned counts[65536], outside, total;

static int find_main(struct dl_phdr_info *info, size_t size, void *data) {
    (void)size;
    (void)data;
    if (info->dlpi_name[0]) return 0;
    image_base = info->dlpi_addr;
    for (int n = 0; n < info->dlpi_phnum; n++) {
        if (info->dlpi_phdr[n].p_type != PT_LOAD) continue;
        uintptr_t low = image_base + info->dlpi_phdr[n].p_vaddr;
        uintptr_t high = low + info->dlpi_phdr[n].p_memsz;
        if (low < image_low) image_low = low;
        if (high > image_high) image_high = high;
    }
    return 1;
}

static void sample(int signal, siginfo_t *info, void *context) {
    (void)signal;
    (void)info;
    uintptr_t pc = ((ucontext_t *)context)->uc_mcontext.gregs[REG_RIP];
    atomic_fetch_add_explicit(&total, 1, memory_order_relaxed);
    if (pc < image_low || pc >= image_high) {
        atomic_fetch_add_explicit(&outside, 1, memory_order_relaxed);
        return;
    }
    uintptr_t key = pc - image_base + 1;
    unsigned slot = (key * UINT64_C(0x9e3779b97f4a7c15)) >> 48;
    for (unsigned n = 0; n < 65536; n++, slot = (slot + 1) & 65535) {
        uintptr_t stored = atomic_load_explicit(&keys[slot], memory_order_relaxed);
        if (!stored || stored == key) {
            atomic_store_explicit(&keys[slot], key, memory_order_relaxed);
            atomic_fetch_add_explicit(&counts[slot], 1, memory_order_relaxed);
            return;
        }
    }
}

__attribute__((constructor)) static void begin(void) {
    dl_iterate_phdr(find_main, NULL);
    struct sigaction action = {0};
    action.sa_sigaction = sample;
    action.sa_flags = SA_SIGINFO;
    sigemptyset(&action.sa_mask);
    if (sigaction(SIGPROF, &action, NULL)) return;
    struct itimerval timer = {{0, 1000}, {0, 1000}};
    setitimer(ITIMER_PROF, &timer, NULL);
}

__attribute__((destructor)) static void finish(void) {
    struct itimerval timer = {0};
    setitimer(ITIMER_PROF, &timer, NULL);
    sigset_t set;
    sigemptyset(&set);
    sigaddset(&set, SIGPROF);
    sigprocmask(SIG_BLOCK, &set, NULL);
    const char *path = getenv("DEFT_SAMPLE_OUT");
    if (!path) return;
    FILE *out = fopen(path, "w");
    if (!out) return;
    fprintf(out, "# total=%u outside_executable=%u\n", total, outside);
    for (unsigned n = 0; n < 65536; n++) {
        uintptr_t key = atomic_load_explicit(&keys[n], memory_order_relaxed);
        if (key) fprintf(out, "%lx\t%u\n", (unsigned long)(key - 1), counts[n]);
    }
    fclose(out);
}
